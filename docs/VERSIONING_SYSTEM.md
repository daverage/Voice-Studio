# Automatic Versioning System

## Overview

The tinyMem project implements an automated versioning system that manages semantic versioning (SemVer) through a combination of build scripts and version tracking. This document explains how the system works and how to adapt it for other languages and platforms.

## Core Components

### 1. Version Storage

The current version is stored in a dedicated file:
- **Location**: `internal/version/version.go`
- **Format**: `var Version = "v0.1.12"`

This file serves as the single source of truth for the current version number.

### 2. Build Scripts

The versioning system is managed by platform-specific build scripts:
- **Unix/Linux/macOS**: `build/build.sh`
- **Windows**: `build/build.bat`

These scripts handle:
- Reading the current version
- Calculating new versions based on SemVer rules
- Injecting version information during compilation
- Creating Git tags and GitHub releases

## How It Works

### Version Calculation

The build system calculates versions based on Git tags:

1. **Get Latest Tag**: The system retrieves the most recent Git tag using `git describe --tags --abbrev=0`
2. **Parse Version**: The tag is parsed into Major.Minor.Patch components
3. **Increment Version**: Depending on the mode (major/minor/patch), the appropriate component is incremented:
   - `major`: Increments major version, resets minor and patch to 0
   - `minor`: Increments minor version, resets patch to 0
   - `patch`: Increments patch version only

### Build Process

During the build process:
1. The calculated version is injected into the binary using linker flags (Go's `-ldflags`)
2. For Go specifically: `-X github.com/andrzejmarczewski/tinyMem/internal/version.Version=vX.Y.Z`
3. Binaries are compiled for multiple platforms (macOS, Linux, Windows) with appropriate naming

### Release Process

When creating a release:
1. The version in the source code is updated
2. Changes are committed to the repository
3. A Git tag is created with the new version
4. The tag and changes are pushed to the remote repository
5. A GitHub release is created with the compiled binaries attached

## Adapting for Other Languages

### Language-Specific Considerations

While the current implementation is in Go, the concepts can be applied to any language:

#### For Compiled Languages (C++, Rust, etc.)
- Store version in a configuration file or header
- Use compiler/linker flags to inject version information
- Adapt the build script to use the appropriate compiler

#### For Interpreted Languages (Python, JavaScript, Ruby)
- Store version in a configuration file (e.g., `setup.py`, `package.json`, `Gemfile`)
- Use build tools to update version files during release
- Include version information in distribution packages

#### For Virtual Machine Languages (Java, .NET)
- Store version in project configuration files (e.g., `pom.xml`, `build.gradle`, `.csproj`)
- Use build systems (Maven, Gradle, MSBuild) to manage version injection

### Platform Adaptation

The build scripts can be adapted for different platforms:

#### Unix-like Systems (Linux/macOS)
- Use shell scripts similar to the provided `build.sh`
- Leverage Git command-line tools
- Use cross-compilation tools if needed (e.g., Docker, zig, musl-cross)

#### Windows
- Use batch files or PowerShell scripts
- Adapt Git commands for Windows environments
- Consider using Windows Subsystem for Linux (WSL) for Unix-style scripting

## Implementation Template

Here's a generic template for implementing this system in any language:

```bash
#!/bin/bash
# Generic Versioning Template

# Determine project root
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$PROJECT_ROOT"

# Get latest tag
LATEST_TAG="$(git describe --tags --abbrev=0 2>/dev/null || echo v0.0.0)"
IFS='.' read -r MAJOR MINOR PATCH <<< "${LATEST_TAG#v}"

# Calculate new version based on mode
case "$MODE" in
  major) ((MAJOR++)); MINOR=0; PATCH=0 ;;
  minor) ((MINOR++)); PATCH=0 ;;
  patch) ((PATCH++)) ;;
esac
NEW_VERSION="v$MAJOR.$MINOR.$PATCH"

# Update version in source code
# (Language-specific implementation needed here)

# Build project with new version
# (Language-specific build commands needed here)

# Create Git tag
git tag -a "$NEW_VERSION" -m "Release $NEW_VERSION"

# Push changes and tag
git push origin main
git push origin "$NEW_VERSION"
```

## Best Practices

1. **Single Source of Truth**: Maintain version in one canonical location
2. **Semantic Versioning**: Follow SemVer principles (MAJOR.MINOR.PATCH)
3. **Automated Updates**: Automatically update version files during release
4. **Git Integration**: Use Git tags to track releases
5. **Platform Compatibility**: Ensure build scripts work across target platforms
6. **Safety Checks**: Verify clean working directory before releasing
7. **Backup Plans**: Have fallback mechanisms if automation fails

## Integration Points

When adapting this system:
- Integrate with your CI/CD pipeline
- Connect to your package registry or distribution system
- Ensure proper access controls for release operations
- Set up notifications for successful releases