@echo off
SETLOCAL EnableDelayedExpansion
TITLE Vector Engine V2 - Auto-Setup & Benchmark
CLS

ECHO ===================================================
ECHO       Vector Engine V2 - Windows System Check
ECHO ===================================================
ECHO.

:: -----------------------------------------------------
:: 1. Check WSL Status
:: -----------------------------------------------------
ECHO [1/3] Checking Windows Subsystem for Linux (WSL)...
wsl --status >nul 2>&1
IF %ERRORLEVEL% NEQ 0 (
    ECHO [!] WSL is not explicitly installed or requires update.
    ECHO     Attempting auto-install...
    wsl --install
    ECHO.
    ECHO [IMPORTANT] System restart may be required after WSL install.
    ECHO If prompted, please restart and run this script again.
    PAUSE
    EXIT /B
) ELSE (
    ECHO [OK] WSL is active.
)

:: -----------------------------------------------------
:: 2. Check Docker Desktop
:: -----------------------------------------------------
ECHO.
ECHO [2/3] Checking Docker Desktop...
docker --version >nul 2>&1
IF %ERRORLEVEL% NEQ 0 (
    ECHO [!] Docker not found.
    ECHO     Downloading Docker Desktop Installer from official source...
    ECHO     (This may take a few minutes depending on connection)
    
    :: Download Installer using PowerShell to %TEMP%
    powershell -Command "Invoke-WebRequest -Uri 'https://desktop.docker.com/win/main/amd64/Docker%20Desktop%20Installer.exe' -OutFile '%TEMP%\DockerInstaller.exe'"
    
    IF EXIST "%TEMP%\DockerInstaller.exe" (
        ECHO [!] Installing Docker Desktop...
        ECHO     Please verify the UAC prompt and follow the installer.
        
        :: Launch Installer and wait
        start /w "" "%TEMP%\DockerInstaller.exe"
        
        ECHO.
        ECHO [ACTION REQUIRED]
        ECHO Please start Docker Desktop from your Start Menu and wait for the engine to initialize.
        ECHO Once Docker is running (whale icon in tray), press any key to continue.
        PAUSE
    ) ELSE (
        ECHO [ERROR] Failed to download Docker Installer.
        PAUSE
        EXIT /B
    )
) ELSE (
    ECHO [OK] Docker is installed.
)

:: -----------------------------------------------------
:: 3. Build & Run
:: -----------------------------------------------------
ECHO.
ECHO [3/3] Launching Vector Engine Container...
ECHO.

:: Check if Docker daemon is actually running
docker info >nul 2>&1
IF %ERRORLEVEL% NEQ 0 (
    ECHO [!] Docker is installed but NOT running.
    ECHO     Please start Docker Desktop and wait for it to initialize.
    PAUSE
    EXIT /B
)

:: Build
ECHO Building Image (v2.0.0)...
docker build -t vector-engine:v2 .
IF %ERRORLEVEL% NEQ 0 (
    ECHO [ERROR] Docker Build failed.
    PAUSE
    EXIT /B
)

:: Run Generator (if data doesn't exist)
ECHO.
ECHO Generating Benchmark Data (100k Vectors)...
docker run --rm -v vector_data:/data vector-engine:v2 generator --num-vectors 100000 --output /data/bench.bin --m 24 --ef 200

:: Run Benchmark
ECHO.
ECHO Running Stress Test...
docker run -it --rm -v vector_data:/data vector-engine:v2 stress_test --index /data/bench.bin --concurrency %NUMBER_OF_PROCESSORS%

ECHO.
ECHO ===================================================
ECHO               Benchmark Complete
ECHO ===================================================
PAUSE
