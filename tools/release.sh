#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

if ! command -v gh >/dev/null 2>&1; then
  echo "❌ GitHub CLI (gh) not installed."
  exit 1
fi

if [[ -n "$(git status -s -- . ':(exclude).tinyMem' ':(exclude)dist')" ]]; then
  echo "ℹ️ Working tree has changes (excluding .tinyMem/dist); they will be included in the release commit."
fi

VERSION="$(rg -n 'version = "' Cargo.toml | head -1 | sed -E 's/.*version = "([^"]+)".*/\1/')"
if [[ -z "${VERSION}" ]]; then
  echo "❌ Could not read version from Cargo.toml."
  exit 1
fi

LIB_VERSION="$(rg -n 'const VERSION' src/lib.rs | sed -E 's/.*"([^"]+)".*/\1/' | tr -d '[:space:]')"
if [[ -z "${LIB_VERSION}" ]]; then
  echo "❌ Could not read VERSION from src/lib.rs."
  exit 1
fi

if [[ "${VERSION}" != "${LIB_VERSION}" ]]; then
  echo "❌ Version mismatch: Cargo.toml=${VERSION} src/lib.rs=${LIB_VERSION}"
  exit 1
fi

COMMIT_MSG="${COMMIT_MSG:-}"
if [[ -z "${COMMIT_MSG}" ]]; then
  read -r -p "Commit message for v${VERSION}: " COMMIT_MSG
fi
if [[ -z "${COMMIT_MSG}" ]]; then
  echo "❌ Commit message required."
  exit 1
fi

DIST_DIR="${PROJECT_ROOT}/dist/v${VERSION}"
rm -rf "${DIST_DIR}"
mkdir -p "${DIST_DIR}"

HOST_TARGET="$(rustc -vV | rg '^host:' | awk '{print $2}')"
TARGETS=("${HOST_TARGET}|macos" "x86_64-pc-windows-msvc|windows")
if [[ "${SKIP_LINUX:-}" != "1" ]]; then
  TARGETS+=("x86_64-unknown-linux-gnu|linux")
fi

for entry in "${TARGETS[@]}"; do
  target="${entry%%|*}"
  os="${entry##*|}"

  echo "🔨 Building ${os} (${target})..."
  if ! rustup target list --installed | rg -qx "${target}"; then
    echo "❌ Missing Rust target ${target}. Install with: rustup target add ${target}"
    exit 1
  fi

  if [[ "${os}" == "linux" ]]; then
    if ! command -v cross >/dev/null 2>&1; then
      echo "❌ cross is not installed. Install with: cargo install cross"
      exit 1
    fi
    CARGO_TARGET_DIR="${PROJECT_ROOT}/target/${os}" \
      CROSS_NO_TOOLCHAIN=1 cross nih-plug bundle vxcleaner --release --target "${target}"
  elif [[ "${os}" == "windows" ]]; then
    if ! command -v xwin >/dev/null 2>&1; then
      echo "❌ xwin is not installed. Install with: cargo install xwin"
      exit 1
    fi
    if [[ -x "/opt/homebrew/opt/llvm/bin/clang-cl" ]]; then
      export PATH="/opt/homebrew/opt/llvm/bin:${PATH}"
    else
      echo "❌ clang-cl not found. Install LLVM and ensure clang-cl is in PATH."
      exit 1
    fi

    if ! command -v lld-link >/dev/null 2>&1; then
      if [[ -x "/opt/homebrew/opt/lld/bin/lld-link" ]]; then
        export PATH="/opt/homebrew/opt/lld/bin:${PATH}"
      elif [[ -x "/opt/homebrew/opt/llvm/bin/lld-link" ]]; then
        export PATH="/opt/homebrew/opt/llvm/bin:${PATH}"
      else
        echo "❌ lld-link not found. Install LLVM/LLD and ensure lld-link is in PATH."
        exit 1
      fi
    fi

    if ! command -v llvm-lib >/dev/null 2>&1; then
      if [[ -x "/opt/homebrew/opt/llvm/bin/llvm-lib" ]]; then
        export PATH="/opt/homebrew/opt/llvm/bin:${PATH}"
      else
        echo "❌ llvm-lib not found. Install LLVM and ensure llvm-lib is in PATH."
        exit 1
      fi
    fi

    XWIN_DIR="${PROJECT_ROOT}/xwin"
    if [[ ! -d "${XWIN_DIR}" ]]; then
      xwin --accept-license splat --output "${XWIN_DIR}"
    fi

    CRT_LIB_DIR="$(find "${XWIN_DIR}" -type d -path '*/crt/lib/x86_64' | head -1)"
    SDK_UM_LIB_DIR="$(find "${XWIN_DIR}" -type d -path '*/um/x86_64' | head -1)"
    SDK_UCRT_LIB_DIR="$(find "${XWIN_DIR}" -type d -path '*/ucrt/x86_64' | head -1)"
    CRT_INCLUDE_DIR="$(find "${XWIN_DIR}" -type d -path '*/crt/include' | head -1)"
    SDK_UM_INCLUDE_DIR="$(find "${XWIN_DIR}" -type d -path '*/um' | head -1)"
    SDK_SHARED_INCLUDE_DIR="$(find "${XWIN_DIR}" -type d -path '*/shared' | head -1)"
    SDK_UCRT_INCLUDE_DIR="$(find "${XWIN_DIR}" -type d -path '*/ucrt' | head -1)"

    if [[ -z "${CRT_LIB_DIR}" || -z "${SDK_UM_LIB_DIR}" || -z "${SDK_UCRT_LIB_DIR}" ]]; then
      echo "❌ xwin layout not found. Check ${XWIN_DIR} contents."
      exit 1
    fi
    if [[ -z "${CRT_INCLUDE_DIR}" || -z "${SDK_UM_INCLUDE_DIR}" || -z "${SDK_SHARED_INCLUDE_DIR}" || -z "${SDK_UCRT_INCLUDE_DIR}" ]]; then
      echo "❌ xwin include layout not found. Check ${XWIN_DIR} contents."
      exit 1
    fi

    LIB="${CRT_LIB_DIR};${SDK_UM_LIB_DIR};${SDK_UCRT_LIB_DIR}"
    INCLUDE="${CRT_INCLUDE_DIR};${SDK_UM_INCLUDE_DIR};${SDK_SHARED_INCLUDE_DIR};${SDK_UCRT_INCLUDE_DIR}"

    LIB="${LIB}" INCLUDE="${INCLUDE}" \
      CC_x86_64_pc_windows_msvc=clang-cl \
      CXX_x86_64_pc_windows_msvc=clang-cl \
      CFLAGS_x86_64_pc_windows_msvc="--target=x86_64-pc-windows-msvc" \
      CXXFLAGS_x86_64_pc_windows_msvc="--target=x86_64-pc-windows-msvc" \
      AR_x86_64_pc_windows_msvc=llvm-lib \
      ARFLAGS_x86_64_pc_windows_msvc="/machine:x64" \
      CARGO_TARGET_X86_64_PC_WINDOWS_MSVC_LINKER=lld-link \
      CARGO_TARGET_DIR="${PROJECT_ROOT}/target/${os}" \
      cargo nih-plug bundle vxcleaner --release --target "${target}"
  else
    CARGO_TARGET_DIR="${PROJECT_ROOT}/target/${os}" \
      cargo nih-plug bundle vxcleaner --release --target "${target}"
  fi

  BUNDLED_DIR="target/${os}/bundled"
  if [[ ! -d "${BUNDLED_DIR}" ]]; then
    echo "❌ Missing bundled output at ${BUNDLED_DIR}"
    exit 1
  fi

  for kind in vst3 clap; do
    bundle_path="$(ls -d "${BUNDLED_DIR}"/vxcleaner."${kind}" 2>/dev/null || true)"
    if [[ -z "${bundle_path}" ]]; then
      echo "❌ Missing ${kind} bundle for ${os} in ${BUNDLED_DIR}"
      exit 1
    fi

    temp_dir="${DIST_DIR}/_${os}_${kind}"
    rm -rf "${temp_dir}"
    mkdir -p "${temp_dir}"

    cp -R "${bundle_path}" "${temp_dir}/"
    cp "src/help.html" "${temp_dir}/help.html"

    if [[ "${os}" == "macos" ]]; then
      ln -s "/Library/Audio/Plug-Ins/VST" "${temp_dir}/VST Folder"
      ln -s "/Library/Audio/Plug-Ins/VST3" "${temp_dir}/VST3 Folder"
    fi

    zip_name="vxcleaner-${os}-${kind}.zip"
    if [[ "${os}" == "macos" ]]; then
      (cd "${temp_dir}" && zip -r -y "../${zip_name}" . >/dev/null)
    else
      (cd "${temp_dir}" && zip -r "../${zip_name}" . >/dev/null)
    fi
    rm -rf "${temp_dir}"
  done
done

git add Cargo.toml Cargo.lock src/lib.rs src/version.rs .gitignore tools/release.sh docs/RELEASE_PROCESS.md CLAUDE.md GEMINI.md QWEN.md
git commit -m "${COMMIT_MSG}"

TAG="v${VERSION}"
git tag -a "${TAG}" -m "${COMMIT_MSG}"

echo "📦 Creating GitHub release ${TAG}..."
gh release create "${TAG}" \
  --title "vxcleaner ${TAG}" \
  --notes "${COMMIT_MSG}" \
  "${DIST_DIR}"/*.zip

echo "✅ Release ${TAG} created."
