# System parity fixtures

Copied from ducktape core shipping components at commit `d48857bb0c6594af2c8bf9b28a0babeffe0b5922` (PR #2579), using `crates/modules/system/<id>/component.wasm`. No separate fixture build.

Guest and native SDK: `16c8e4fcae7d2f998b01825af6eebf0fffb4c848`. Guest source and native host/statesync: core `d91dbe32c9d6275383f4ae742acf7a3b94beac5b`. The artifact commit records the coordinated rebuild of that frozen source.

Module WIT SHA-256: `0e0845bedda9e93c4d6ffd5e06a0d0abc9a7468ec2ac4a060cbb38bf4f182280`.

Refresh from canonical committed shipping components whenever the coordinated guest contract changes; record source/artifact revisions and hashes here.

| File | SHA-256 |
| --- | --- |
| identity.component.wasm | 01818d9739faa91e0798db02890a7e93b90cf39891a02fa3a68e2eba051855c7 |
| dispatch.component.wasm | 43900272335b5589ea39ec66a5a511774e5cd382426cc60c3d33748cff9c0c42 |
| attribution.component.wasm | be509eb4bccf31e447b9962f88ef4992ca356d3cb77ed8050350cd596cf8f6c3 |
