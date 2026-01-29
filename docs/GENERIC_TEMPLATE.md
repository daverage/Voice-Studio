# Generic Versioning and Publishing Template

## Overview

This template provides a universal framework for implementing automatic versioning and publishing systems that can be adapted to any programming language, platform, or distribution method.

## Template Structure

```
project-root/
├── build/
│   ├── build.sh          # Unix/Linux/macOS build script
│   ├── build.bat         # Windows build script
│   └── templates/        # Template files for different languages
│       ├── go-template/
│       ├── python-template/
│       ├── javascript-template/
│       └── java-template/
├── internal/
│   └── version/
│       └── version.go    # Version storage (language-specific)
├── .github/
│   └── workflows/
│       └── release.yml   # CI/CD workflow for automated releases
└── docs/
    ├── VERSIONING.md     # Versioning documentation
    └── PUBLISHING.md     # Publishing documentation
```

## Universal Build Script Template

### Unix/Linux/macOS (build.sh)

```bash
#!/bin/bash
# Universal Build & Release Script Template
# Builds platform binaries and handles full release lifecycle if requested.
# Usage:
#   ./build/build.sh                 (Build only)
#   ./build/build.sh [major|minor|patch] (Full release cycle)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

OUT_DIR="build/releases"
mkdir -p "$OUT_DIR"

# ------------------------------------------------------------
# Determine if we are in Release Mode
# ------------------------------------------------------------
MODE="${1:-}"
IS_RELEASE=false
if [[ "$MODE" == "major" || "$MODE" == "minor" || "$MODE" == "patch" ]]; then
  IS_RELEASE=true
fi

# ------------------------------------------------------------
# Safety checks for Release Mode
# ------------------------------------------------------------
if [ "$IS_RELEASE" = true ]; then
  command -v gh >/dev/null || {
    echo "❌ GitHub CLI (gh) not installed. Required for releases."
    exit 1
  }
  
  # Check if working directory is clean
  if [[ -n $(git status -s) ]]; then
    echo "❌ Working directory is not clean. Commit or stash changes before releasing."
    exit 1
  fi
fi

# ------------------------------------------------------------
# Version calculation
# ------------------------------------------------------------
LATEST_TAG="$(git describe --tags --abbrev=0 2>/dev/null || echo v0.0.0)"
IFS='.' read -r MAJOR MINOR PATCH <<< "${LATEST_TAG#v}"

if [ "$IS_RELEASE" = true ]; then
  case "$MODE" in
    major) ((MAJOR++)); MINOR=0; PATCH=0 ;;
    minor) ((MINOR++)); PATCH=0 ;;
    patch) ((PATCH++)) ;;
  esac
  VERSION="v$MAJOR.$MINOR.$PATCH"

  echo "🚀 Preparing Release: $VERSION (Current: $LATEST_TAG)"
else
  # Read current version from code for standard build
  # LANGUAGE-SPECIFIC: Adjust this line for your language
  VERSION="$(grep -E 'version:' pyproject.toml | cut -d'"' -f2 2>/dev/null || echo "$LATEST_TAG")"
  echo "Building project version: $VERSION"
fi

# ------------------------------------------------------------
# Build Logic
# LANGUAGE-SPECIFIC: Customize this section for your build process
# ------------------------------------------------------------

# Clear previous releases
rm -rf "$OUT_DIR"/*

if [ "$IS_RELEASE" = true ]; then
  echo
  read -p "Build successful. Commit message for $VERSION: " COMMIT_MSG
  if [[ -z "$COMMIT_MSG" ]]; then
    echo "❌ Commit message required."
    exit 1
  fi

  # LANGUAGE-SPECIFIC: Update version in source code
  # Example for Python (pyproject.toml):
  # sed -i.bak "s/version = \"[^\"]*\"/version = \"$VERSION\"/" pyproject.toml
  # rm pyproject.toml.bak

  echo "📝 Updating version file..."
  # PLACEHOLDER: Insert command to update version in your project
  
  echo "💾 Committing changes..."
  git add .
  git commit -m "$COMMIT_MSG (Release $VERSION)" || echo "No code changes to commit."

  # Check if tag exists
  if git rev-parse "$VERSION" >/dev/null 2>&1; then
    echo "⚠️  Tag $VERSION already exists locally. Updating..."
    git tag -d "$VERSION"
  fi

  echo "🏷️  Tagging $VERSION..."
  git tag -a "$VERSION" -m "$COMMIT_MSG"

  echo "⬆️  Pushing to origin..."
  git push origin main
  git push origin "$VERSION" --force

  echo "📦 Creating GitHub Release..."
  if gh release view "$VERSION" >/dev/null 2>&1; then
    echo "⚠️  Release $VERSION already exists. Uploading assets..."
    gh release upload "$VERSION" "$OUT_DIR"/* --clobber
  else
    gh release create "$VERSION" \
      --title "Project Name $VERSION" \
      --notes "$COMMIT_MSG" \
      "$OUT_DIR"/*
  fi

  echo "✅ Release $VERSION processed successfully!"
else
  echo
  echo "Build complete. Artifacts in $OUT_DIR"
fi
```

### Windows (build.bat)

```batch
@echo off
REM Universal Build & Release Script Template (Windows)
REM Builds platform binaries and handles full release lifecycle if requested.
REM Usage:
REM   .\build\build.bat                 (Build only)
REM   .\build\build.bat [major|minor|patch] (Full release cycle)

setlocal enabledelayedexpansion

REM ------------------------------------------------
REM Resolve project root
REM ------------------------------------------------
set SCRIPT_DIR=%~dp0
set PROJECT_ROOT=%SCRIPT_DIR%..
cd /d "%PROJECT_ROOT%"

set OUT_DIR=build\releases
if not exist "%OUT_DIR%" mkdir "%OUT_DIR%"

REM ------------------------------------------------
REM Determine if we are in Release Mode
REM ------------------------------------------------
set MODE=%1
set IS_RELEASE=false
if "%MODE%"=="major" set IS_RELEASE=true
if "%MODE%"=="minor" set IS_RELEASE=true
if "%MODE%"=="patch" set IS_RELEASE=true

REM ------------------------------------------------
REM Safety checks for Release Mode
REM ------------------------------------------------
if "%IS_RELEASE%"=="true" (
    REM Check if working directory is clean
    git status -s > temp_status.txt
    set /p STATUS=<temp_status.txt
    del temp_status.txt
    if not "!STATUS!"=="" (
        echo ❌ Error: Working directory is not clean. Commit or stash changes before releasing.
        exit /b 1
    )

    where gh >nul 2>nul
    if errorlevel 1 (
        echo ❌ Error: GitHub CLI (gh) not installed. Required for releases.
        exit /b 1
    )
)

REM ------------------------------------------------
REM Get latest tag
REM ------------------------------------------------
for /f "tokens=*" %%i in ('git describe --tags --abbrev^=0 2^>nul') do set LATEST_TAG=%%i
if "%LATEST_TAG%"=="" set LATEST_TAG=v0.0.0

REM ------------------------------------------------
REM Version calculation
REM ------------------------------------------------
if "%IS_RELEASE%"=="true" (
    set VERSION_STR=%LATEST_TAG:~1%
    for /f "tokens=1,2,3 delims=." %%a in ("!VERSION_STR!") do (
        set MAJOR=%%a
        set MINOR=%%b
        set PATCH=%%c
    )

    if "%MODE%"=="major" (
        set /a MAJOR+=1
        set MINOR=0
        set PATCH=0
    ) else if "%MODE%"=="minor" (
        set /a MINOR+=1
        set PATCH=0
    ) else (
        set /a PATCH+=1
    )

    set VERSION=v!MAJOR!.!MINOR!.!PATCH!
    echo 🚀 Preparing Release: !VERSION! (Current: %LATEST_TAG%)
) else (
    REM Read current version from code
    REM LANGUAGE-SPECIFIC: Adjust this line for your language
    REM Example for Python:
    REM for /f "tokens=2 delims==" %%v in ('findstr /R "version =" pyproject.toml') do (
    REM     set VERSION=%%~v
    REM )
    set VERSION=!VERSION:"=!
    if "!VERSION!"=="" set VERSION=%LATEST_TAG%
    echo Building project version: !VERSION!
)

REM ------------------------------------------------
REM Build Logic
REM LANGUAGE-SPECIFIC: Customize this section for your build process
REM ------------------------------------------------

REM Clear previous releases
if exist "%OUT_DIR%\*" del /q "%OUT_DIR%\*"

REM PLACEHOLDER: Insert your build commands here

REM ------------------------------------------------
REM Finalize Release
REM ------------------------------------------------
if "%IS_RELEASE%"=="true" (
    echo.
    set /p COMMIT_MSG=Build successful. Commit message for !VERSION!:
    if "!COMMIT_MSG!"=="" (
        echo ❌ Error: Commit message required.
        exit /b 1
    )

    REM LANGUAGE-SPECIFIC: Update version in source code
    REM Example for Python (pyproject.toml):
    REM powershell -Command ^
    REM   "(Get-Content pyproject.toml) ^
    REM    -replace 'version = \"[^\"]*\"', 'version = \"!VERSION!\"' ^
    REM    | Set-Content pyproject.toml"

    echo 📝 Updating version file...
    REM PLACEHOLDER: Insert command to update version in your project

    echo 💾 Committing changes...
    git add .
    git commit -m "!COMMIT_MSG! (Release !VERSION!)" || echo No changes to commit.

    REM Check if tag exists
    git rev-parse !VERSION! >nul 2>nul
    if not errorlevel 1 (
        echo ⚠️  Tag !VERSION! already exists locally. Updating...
        git tag -d !VERSION!
    )

    echo 🏷️  Tagging !VERSION!...
    git tag -a "!VERSION!" -m "!COMMIT_MSG!"

    echo ⬆️  Pushing to origin...
    git push origin main
    git push origin "!VERSION!" --force

    echo 📦 Creating GitHub Release...
    gh release view "!VERSION!" >nul 2>nul
    if not errorlevel 1 (
        echo ⚠️  Release !VERSION! already exists. Uploading assets...
        gh release upload "!VERSION!" "%OUT_DIR%\*" --clobber
    ) else (
        gh release create "!VERSION!" ^
          --title "Project Name !VERSION!" ^
          --notes "!COMMIT_MSG!" ^
          "%OUT_DIR%\*"
    )

    echo.
    echo ✅ Release !VERSION! processed successfully!
) else (
    echo.
    echo Build complete. Artifacts in %OUT_DIR%
)

exit /b 0
```

## Language-Specific Templates

### Python Template

```python
# setup.py or pyproject.toml
[project]
name = "your-project"
version = "0.1.0"  # This will be updated during release
```

```bash
# For build.sh - Python-specific build commands
python -m build
twine upload dist/*
```

### JavaScript/Node.js Template

```json
// package.json
{
  "name": "your-project",
  "version": "0.1.0"  // This will be updated during release
}
```

```bash
# For build.sh - JavaScript-specific build commands
npm run build
npm publish
```

### Java Template

```xml
<!-- pom.xml -->
<project>
  ...
  <version>0.1.0</version>  <!-- This will be updated during release -->
  ...
</project>
```

```bash
# For build.sh - Java-specific build commands
mvn clean package
mvn deploy
```

## CI/CD Integration Template

### GitHub Actions (.github/workflows/release.yml)

```yaml
name: Release

on:
  push:
    branches: [main]

jobs:
  release:
    runs-on: ubuntu-latest
    if: contains(github.event.head_commit.message, '(Release')
    
    steps:
    - uses: actions/checkout@v3
      with:
        fetch-depth: 0
        
    - name: Set up Go
      uses: actions/setup-go@v4
      with:
        go-version: '1.21'
        
    - name: Install dependencies
      run: |
        sudo apt-get update
        sudo apt-get install -y gcc-aarch64-linux-gnu libc6-dev-arm64-cross
        
    - name: Run build script
      run: ./build/build.sh
      
    - name: Create Release
      uses: actions/create-release@v1
      env:
        GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}
      with:
        tag_name: ${{ env.NEW_VERSION }}
        release_name: Release ${{ env.NEW_VERSION }}
        draft: false
        prerelease: false
```

## Customization Guide

### For Different Distribution Channels

#### For npm Registry
1. Update package.json version
2. Run `npm publish`

#### For PyPI
1. Update setup.py/pyproject.toml version
2. Run `python -m twine upload dist/*`

#### For Maven Central
1. Update pom.xml version
2. Run `mvn deploy`

### For Different Platforms

#### For Docker Images
1. Build Docker image with version tag
2. Push to container registry

#### For Mobile Apps
1. Update app manifest with version
2. Submit to app stores

## Best Practices

1. **Consistency**: Use the same version format across all components
2. **Automation**: Automate as much of the process as possible
3. **Verification**: Verify builds before publishing
4. **Security**: Secure credentials and signing keys
5. **Documentation**: Keep documentation updated with each release
6. **Testing**: Run tests before creating releases
7. **Notifications**: Notify stakeholders of new releases

## Troubleshooting

### Common Issues

1. **Permission Errors**: Ensure proper permissions for publishing
2. **Authentication Failures**: Verify credentials are correctly set
3. **Build Failures**: Check dependencies and build environment
4. **Version Conflicts**: Ensure version numbers are unique

### Recovery Procedures

1. **Failed Release**: Roll back to previous version if needed
2. **Bad Assets**: Recreate and re-upload assets
3. **Tag Issues**: Delete and recreate Git tags if necessary
4. **Registry Problems**: Contact registry support if needed