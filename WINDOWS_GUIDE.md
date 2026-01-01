# 🪟 Run Vector Engine on Windows 11

The best way to run **Vector Engine V2** on Windows is using **Docker Desktop** (which uses WSL2 under the hood). This preserves all the Linux-specific optimizations (AVX2, Mmap, Huge Pages).

### Prerequisites
1.  **Install Docker Desktop**: [Download Here](https://www.docker.com/products/docker-desktop/)
2.  **Ensure WSL2 is enabled**: Docker usually handles this automatically.

---

### Method 1: The One-Click Script (Recommended)
1.  Double-click `start_windows.bat` included in this release.
2.  It will automatically:
    *   Pull the official V2 image.
    *   Generate a 100k vector index inside the container.
    *   Run the Stress Test dashboard.

---

### Method 2: Manual (PowerShell / Command Prompt)

Open PowerShell and run the following commands:

**1. Pull the Image**
```powershell
docker build -t vector-engine .
```
*(Or pull from registry if you pushed it)*

**2. Run the Benchmark**
```powershell
docker run -it --rm vector-engine stress_test --index /data/benchmark.bin --concurrency 8
```

> **Note**: For 1 Million vectors on Windows, allocate at least 4GB RAM to Docker in settings.
