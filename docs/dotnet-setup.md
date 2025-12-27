# .NET SDK Setup

This guide covers installing the required .NET SDK for GitSpace interop work, along with validation steps.

## Required SDK Version
- **.NET SDK 8.x** (latest patch release).

## OS Prerequisites
- **Windows**: Windows 10/11 with PowerShell and an available package manager (Winget recommended).
- **macOS**: macOS 12+ with Homebrew installed.
- **Linux**: A supported distro (Ubuntu/Debian/Fedora/openSUSE) with `curl` or `wget`.

## Official Installation Commands

### Windows (Winget)
```powershell
winget install Microsoft.DotNet.SDK.8
```

### macOS (Homebrew)
```bash
brew install --cask dotnet-sdk
```

### Linux (Microsoft install script)
```bash
wget https://dot.net/v1/dotnet-install.sh -O dotnet-install.sh
chmod +x dotnet-install.sh
./dotnet-install.sh --channel 8.0
```

> If you install to a custom location on Linux, set `DOTNET_ROOT` and add it to your `PATH`.

## Verify the Install
```bash
dotnet --info
```
