# Risk Register

| ID | Risk Description | Impact | Probability | Mitigation Strategy | Implementation Status |
|----|------------------|--------|-------------|---------------------|-----------------------|
| **R01** | **File Corruption**<br>Index file modified or truncated externally. | Critical | Low | **Pre-Flight Checks**: Validate file size against header. Magic byte verification. | ✅ Implemented |
| **R02** | **Memory Safety (Unsafe SIMD)**<br>Segfault due to misaligned reads in AVX2 kernels. | Critical | Low | **Bounds Checking**: Ensure vectors are correct length before calling unsafe intrinsics. | ✅ Implemented |
| **R03** | **Endianness Mismatch**<br>Index saved on Little Endian, loaded on Big Endian. | High | Low | **Format Spec**: Enforce Little Endian in `save`/`load`. | ⚠️ Partial (Implicit) |
| **R04** | **Data Theft**<br>Attacker reads raw vector data from disk. | High | Medium | **Obfuscation**: Implement XOR scrambling or encryption. | ❌ Pending |
| **R05** | **DoS via Large Allocation**<br>Malicious header specifies huge counts, causing OOM. | High | Low | **Sanity Checks**: Limit max elements/dimensions in loader. | ⚠️ Partial |
| **R06** | **Concurrency Panic**<br>Race conditions during parallel build. | Medium | Low | **Rust Safety**: `rayon` and `RwLock` usage guarantees thread safety. | ✅ Implemented |

## System Health Monitor
The system includes a `Diagnostics` module to actively monitor these risks at runtime.
