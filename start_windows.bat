@echo off
TITLE Vector Engine V2 - Windows Launcher
CLS

ECHO ===================================================
ECHO       Vector Engine V2 - Windows Benchmark
ECHO ===================================================
ECHO.
ECHO This script uses Docker (WSL2) to run the Linux-optimized engine.
ECHO.

WHERE docker >nul 2>nul
IF %ERRORLEVEL% NEQ 0 (
    ECHO [ERROR] Docker is not installed or not in PATH.
    ECHO Please install Docker Desktop: https://www.docker.com/products/docker-desktop/
    PAUSE
    EXIT /B
)

ECHO [1/3] Building Docker Image (Local)...
docker build -t vector-engine:v2 .
IF %ERRORLEVEL% NEQ 0 (
    ECHO [ERROR] Build failed.
    PAUSE
    EXIT /B
)

ECHO.
ECHO [2/3] Generating Benchmark Data (100k Vectors)...
ECHO (This runs inside the container)
ECHO.
docker run --rm -v vector_data:/data vector-engine:v2 generator --num-vectors 100000 --output /data/bench.bin --m 24 --ef 200

ECHO.
ECHO [3/3] Running Stress Test Dashboard...
ECHO.
docker run -it --rm -v vector_data:/data vector-engine:v2 stress_test --index /data/bench.bin --concurrency %NUMBER_OF_PROCESSORS%

ECHO.
ECHO Benchmark Complete.
PAUSE
