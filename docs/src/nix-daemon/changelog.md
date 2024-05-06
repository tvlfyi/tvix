

## Nix version protocol

| Nix version     | Protocol |
| --------------- | -------- |
| 0.11            | 1.02     |
| 0.12            | 1.04     |
| 0.13            | 1.05     |
| 0.14            | 1.05     |
| 0.15            | 1.05     |
| 0.16            | 1.06     |
| 1.0             | 1.10     |
| 1.1             | 1.11     |
| 1.2             | 1.12     |
| 1.3 - 1.5.3     | 1.13     |
| 1.6 - 1.10      | 1.14     |
| 1.11 - 1.11.16  | 1.15     |
| 2.0 - 2.0.4     | 1.20     |
| 2.1 - 2.3.18    | 1.21     |
| 2.4 - 2.6.1     | 1.32     |
| 2.7.0           | 1.33     |
| 2.8.0 - 2.14.1  | 1.34     |
| 2.15.0 - 2.19.4 | 1.35     |
| 2.20.0 - 2.22.0 | 1.37     |

In commit [be64fbb501][be64fbb501] support was droped for protocol versions older than 1.10.
This happened when the protocol was between 1.17 and 1.18 and was released with Nix 2.0.
So this means that any version of Nix 2.x can't talk to Nix 0.x.

## Operation History

| Op              | Id | Commit         | Protocol | Nix Version | Notes |
| --------------- | -- | -------------- | -------- | ----------- | ----- |
| *Quit           | 0  | [a711689368][a711689368] || 0.11 | Became dead code in [7951c3c54][7951c3c54] (Nix 0.11) and removed in [d3c61d83b][d3c61d83b] (Nix 1.8) |
| IsValidPath     | 1  | [a711689368][a711689368] || 0.11 ||
| HasSubstitutes  | 3  | [0565b5f2b3][0565b5f2b3] || 0.11 ||
| QueryPathHash   | 4  | [0565b5f2b3][0565b5f2b3] || 0.11 | Obsolete [e0204f8d46][e0204f8d46]<br>Nix 2.0 Protocol 1.16 |
| QueryReferences | 5  | [0565b5f2b3][0565b5f2b3] || 0.11 | Obsolete [e0204f8d46][e0204f8d46]<br>Nix 2.0 Protocol 1.16 |
| QueryReferrers  | 6  | [0565b5f2b3][0565b5f2b3] || 0.11 ||
| AddToStore      | 7  | [0263279071][0263279071] || 0.11 ||
| AddTextToStore  | 8  | [0263279071][0263279071] || 0.11 | Obsolete [c602ebfb34][c602ebfb34]<br>Nix 2.4 Protocol 1.25 |
| BuildPaths      | 9  | [0565b5f2b3][0565b5f2b3] || 0.11 ||
| EnsurePath      | 10 | [0565b5f2b3][0565b5f2b3] || 0.11 ||
| AddTempRoot     | 11 | [e25fad691a][e25fad691a] || 0.11 ||
| AddIndirectRoot | 12 | [74033a844f][74033a844f] || 0.11 ||
| SyncWithGC      | 13 | [e25fad691a][e25fad691a] || 0.11 | Obsolete [9947f1646a][9947f1646a]<br> Nix 2.5.0 Protocol 1.32 |
| FindRoots       | 14 | [29cf434a35][29cf434a35] || 0.11 ||
| *CollectGarbage | 15 | [a9c4f66cfb][a9c4f66cfb] || 0.11 | Removed [a72709afd8][a72709afd8]<br>Nix 0.12 Protocol 1.02 |
| ExportPath      | 16 | [0f5da8a83c][0f5da8a83c] || 0.11 | Obsolete [538a64e8c3][538a64e8c3]<br>Nix 2.0 Protocol 1.17 |
| *ImportPath     | 17 | [0f5da8a83c][0f5da8a83c] || 0.11 | Removed [273b288a7e][273b288a7e]<br>Nix 1.0 Protocol 1.09 |
| QueryDeriver    | 18 | [6d1a1191b0][6d1a1191b0] || 0.11 | Obsolete [e0204f8d46][e0204f8d46]<br>Nix 2.0 Protocol 1.16 |
| SetOptions      | 19 | [f3441e6122][f3441e6122] || 0.11 ||
| CollectGarbage              | 20 | [a72709afd8][a72709afd8] | 1.02  | 0.12   ||
| QuerySubstitutablePathInfo  | 21 | [03427e76f1][03427e76f1] | 1.02  | 0.12   ||
| QueryDerivationOutputs      | 22 | [e42401ee7b][e42401ee7b] | 1.05  | 1.0    | Obsolete [d38f860c3e][d38f860c3e]<br>Nix 2.4 Protocol 1.22* |
| QueryAllValidPaths          | 23 | [24035b98b1][24035b98b1] | 1.05  | 1.0    ||
| *QueryFailedPaths            | 24 | [f92c9a0ac5][f92c9a0ac5] | 1.05  | 1.0    | Removed [8cffec848][8cffec848]<br>Nix 2.0 Protocol 1.16 |
| *ClearFailedPaths            | 25 | [f92c9a0ac5][f92c9a0ac5] | 1.05  | 1.0    | Removed [8cffec848][8cffec848]<br>Nix 2.0 Protocol 1.16 |
| QueryPathInfo               | 26 | [1db6259076][1db6259076] | 1.06  | 1.0    ||
| ImportPaths                 | 27 | [273b288a7e][273b288a7e] | 1.09  | 1.0    | Obsolete [538a64e8c3][538a64e8c3]<br>Nix 2.0 Protocol 1.17 |
| QueryDerivationOutputNames  | 28 | [af2e53fd48][af2e53fd48]<br>([194d21f9f6][194d21f9f6]) | 1.08      | 1.0 | Obsolete<br>[045b07200c][045b07200c]<br>Nix 2.4 Protocol 1.21 |
| QueryPathFromHashPart       | 29 | [ccc52adfb2][ccc52adfb2] | 1.11  | 1.1    ||
| QuerySubstitutablePathInfos | 30 | [eb3036da87][eb3036da87] | 1.12* | 1.2    ||
| QueryValidPaths             | 31 | [58ef4d9a95][58ef4d9a95] | 1.12  | 1.2    ||
| QuerySubstitutablePaths     | 32 | [09a6321aeb][09a6321aeb] | 1.12  | 1.2    ||
| QueryValidDerivers          | 33 | [2754a07ead][2754a07ead] | 1.13* | 1.3    ||
| OptimiseStore               | 34 | [8fb8c26b6d][2754a07ead] | 1.14  | 1.8    ||
| VerifyStore                 | 35 | [b755752f76][b755752f76] | 1.14  | 1.9    ||
| BuildDerivation             | 36 | [71a5161365][71a5161365] | 1.14  | 1.10   ||
| AddSignatures               | 37 | [d0f5719c2a][d0f5719c2a] | 1.16  | 2.0    ||
| NarFromPath                 | 38 | [b4b5e9ce2f][b4b5e9ce2f] | 1.17  | 2.0    ||
| AddToStoreNar               | 39 | [584f8a62de][584f8a62de] | 1.17  | 2.0    ||
| QueryMissing                | 40 | [ba20730b3f][ba20730b3f] | 1.19* | 2.0    ||
| QueryDerivationOutputMap    | 41 | [d38f860c3e][d38f860c3e] | 1.22* | 2.4    ||
| RegisterDrvOutput           | 42 | [58cdab64ac][58cdab64ac] | 1.27  | 2.4    ||
| QueryRealisation            | 43 | [58cdab64ac][58cdab64ac] | 1.27  | 2.4    ||
| AddMultipleToStore          | 44 | [fe1f34fa60][fe1f34fa60] | 1.32* | 2.4    ||
| AddBuildLog                 | 45 | [4dda1f92aa][4dda1f92aa] | 1.32  | 2.6.0  ||
| BuildPathsWithResults       | 46 | [a4604f1928][a4604f1928] | 1.34* | 2.8.0  ||
| AddPermRoot                 | 47 | [226b0f3956][226b0f3956] | 1.36* | 2.20.0 ||

Notes: Ops that start with * have been removed.
Protocol version that ends with * was bumped while adding that operation. Otherwise protocol version referes to the protocol version at the time the operation was added (so only at the next protocol version can you assume the operation is present/removed/obsolete since it was added/removed/obsoleted between protocol versions).

## Protocol version change log

- 1.01 [f3441e6122][f3441e6122] Initial Version
- 1.02 [c370755583][c370755583] Use build hook
- 1.03 [db4f4a8425][db4f4a8425] Backward compatibility check
- 1.04 [96598e7b06][96598e7b06] SetOptions buildVerbosity
- 1.05 [60ec75048a][60ec75048a] SetOptions useAtime & maxAtime
- 1.06 [6846ed8b44][6846ed8b44] SetOptions buildCores
- 1.07 [bdf089f463][bdf089f463] QuerySubstitutablePathInfo narSize
- 1.08 [b1eb252172][b1eb252172] STDERR_ERROR exit status
- 1.09 [e0bd307802][e0bd307802] ImportPath not supported on versions older than 1.09
- 1.10 [db5b86ef13][db5b86ef13] SetOptions build-use-substitutess
- 1.11 [4bc4da331a][4bc4da331a] open connection reserveSpace
- 1.12 [eb3036da87][eb3036da87] Implement QuerySubstitutablePathInfos
- 1.13 [2754a07ead][2754a07ead] Implement QueryValidDerivers
- 1.14 [a583a2bc59][a583a2bc59] open connection cpu affinity
- 1.15 [d1e3bf01bc][d1e3bf01bc] BuildPaths buildMode
- 1.16 [9cee600c88][9cee600c88] QueryPathInfo ultimate & sigs
- 1.17 [ddea253ff8][ddea253ff8] QueryPathInfo returns valid bool
- 1.18 [4b8f1b0ec0][4b8f1b0ec0] Select between AddToStoreNar and ImportPaths
- 1.19 [ba20730b3f][ba20730b3f] Implement QueryMissing
- 1.20 [cfc8132391][cfc8132391] Don't send activity and result logs to old clients
- 1.21 [6185d25e52][6185d25e52] AddToStoreNar uses TunnelLogger for data
- 1.22 [d38f860c3e][d38f860c3e] Implement QueryDerivationOutputMap and obsolete QueryDerivationOutputs
- 1.23 [4c0077a07d][4c0077a07d] AddToStoreNar uses FramedSink/-Source for data
- 1.24 [5ccd94501d][5ccd94501d] Allow trustless building of CA derivations
- 1.25 [e34fe47d0c][e34fe47d0c] New implementation of AddToStore
- 1.26 [c43e882f54][c43e882f54] STDERR_ERROR serialize exception
- 1.27 [3a63fc6cd5][3a63fc6cd5] QueryValidPaths substitute flag
- 1.28 [27b5747ca7][27b5747ca7] BuildDerivation returns builtOutputs
- 1.29 [9d309de0de][9d309de0de] BuildDerivation returns timesBuilt, isNonDeterministic, startTime & stopTime
- 1.30 [e5951a6b2f][e5951a6b2f] Bump version number for DerivedPath changes
- 1.31 [a8416866cf][a8416866cf] RegisterDrvOutput & QueryRealisation send realisations as JSON
- 1.32 [fe1f34fa60][fe1f34fa60] Implement AddMultipleToStore
- 1.33 [35dbdbedd4][35dbdbedd4] open connection sends nix version
- 1.34 [a4604f1928][a4604f1928] Implement BuildPathsWithResults
- 1.35 [9207f94582][9207f94582] open connection sends trusted option
- 1.36 [226b0f3956][226b0f3956] Implement AddPermRoot
- 1.37 [1e3d811840][1e3d811840] Serialize BuildResult send cpuUser & cpuSystem



[0263279071]: https://github.com/NixOS/nix/commit/0263279071
[03427e76f1]: https://github.com/NixOS/nix/commit/03427e76f1
[045b07200c]: https://github.com/NixOS/nix/commit/045b07200c
[0565b5f2b3]: https://github.com/NixOS/nix/commit/0565b5f2b3
[09a6321aeb]: https://github.com/NixOS/nix/commit/09a6321aeb
[0f5da8a83c]: https://github.com/NixOS/nix/commit/0f5da8a83c
[194d21f9f6]: https://github.com/NixOS/nix/commit/194d21f9f6
[1db6259076]: https://github.com/NixOS/nix/commit/1db6259076
[1e3d811840]: https://github.com/NixOS/nix/commit/1e3d811840
[24035b98b1]: https://github.com/NixOS/nix/commit/24035b98b1
[226b0f3956]: https://github.com/NixOS/nix/commit/226b0f3956
[273b288a7e]: https://github.com/NixOS/nix/commit/273b288a7e
[2754a07ead]: https://github.com/NixOS/nix/commit/2754a07ead
[27b5747ca7]: https://github.com/NixOS/nix/commit/27b5747ca7
[29cf434a35]: https://github.com/NixOS/nix/commit/29cf434a35
[35dbdbedd4]: https://github.com/NixOS/nix/commit/35dbdbedd4
[3a63fc6cd5]: https://github.com/NixOS/nix/commit/3a63fc6cd5
[4b8f1b0ec0]: https://github.com/NixOS/nix/commit/4b8f1b0ec0
[4bc4da331a]: https://github.com/NixOS/nix/commit/4bc4da331a
[4c0077a07d]: https://github.com/NixOS/nix/commit/4c0077a07d
[4dda1f92aa]: https://github.com/NixOS/nix/commit/4dda1f92aa
[538a64e8c3]: https://github.com/NixOS/nix/commit/538a64e8c3
[584f8a62de]: https://github.com/NixOS/nix/commit/584f8a62de
[58cdab64ac]: https://github.com/NixOS/nix/commit/58cdab64ac
[58ef4d9a95]: https://github.com/NixOS/nix/commit/58ef4d9a95
[5ccd94501d]: https://github.com/NixOS/nix/commit/5ccd94501d
[60ec75048a]: https://github.com/NixOS/nix/commit/60ec75048a
[6185d25e52]: https://github.com/NixOS/nix/commit/6185d25e52
[6846ed8b44]: https://github.com/NixOS/nix/commit/6846ed8b44
[6d1a1191b0]: https://github.com/NixOS/nix/commit/6d1a1191b0
[71a5161365]: https://github.com/NixOS/nix/commit/71a5161365
[74033a844f]: https://github.com/NixOS/nix/commit/74033a844f
[7951c3c54]: https://github.com/NixOS/nix/commit/7951c3c54
[8cffec848]: https://github.com/NixOS/nix/commit/8cffec848
[8fb8c26b6d]: https://github.com/NixOS/nix/commit/8fb8c26b6d
[9207f94582]: https://github.com/NixOS/nix/commit/9207f94582
[96598e7b06]: https://github.com/NixOS/nix/commit/96598e7b06
[9947f1646a]: https://github.com/NixOS/nix/commit/9947f1646a
[9cee600c88]: https://github.com/NixOS/nix/commit/9cee600c88
[9d309de0de]: https://github.com/NixOS/nix/commit/9d309de0de
[a4604f1928]: https://github.com/NixOS/nix/commit/a4604f1928
[a583a2bc59]: https://github.com/NixOS/nix/commit/a583a2bc59
[a711689368]: https://github.com/NixOS/nix/commit/a711689368
[a72709afd8]: https://github.com/NixOS/nix/commit/a72709afd8
[a8416866cf]: https://github.com/NixOS/nix/commit/a8416866cf
[a9c4f66cfb]: https://github.com/NixOS/nix/commit/a9c4f66cfb
[af2e53fd48]: https://github.com/NixOS/nix/commit/af2e53fd48
[b1eb252172]: https://github.com/NixOS/nix/commit/b1eb252172
[b4b5e9ce2f]: https://github.com/NixOS/nix/commit/b4b5e9ce2f
[b755752f76]: https://github.com/NixOS/nix/commit/b755752f76
[ba20730b3f]: https://github.com/NixOS/nix/commit/ba20730b3f
[bdf089f463]: https://github.com/NixOS/nix/commit/bdf089f463
[be64fbb501]: https://github.com/NixOS/nix/commit/be64fbb501
[c370755583]: https://github.com/NixOS/nix/commit/c370755583
[c43e882f54]: https://github.com/NixOS/nix/commit/c43e882f54
[c602ebfb34]: https://github.com/NixOS/nix/commit/c602ebfb34
[ccc52adfb2]: https://github.com/NixOS/nix/commit/ccc52adfb2
[cfc8132391]: https://github.com/NixOS/nix/commit/cfc8132391
[d0f5719c2a]: https://github.com/NixOS/nix/commit/d0f5719c2a
[d1e3bf01bc]: https://github.com/NixOS/nix/commit/d1e3bf01bc
[d38f860c3e]: https://github.com/NixOS/nix/commit/d38f860c3e
[d3c61d83b]: https://github.com/NixOS/nix/commit/d3c61d83b
[db4f4a8425]: https://github.com/NixOS/nix/commit/db4f4a8425
[db5b86ef13]: https://github.com/NixOS/nix/commit/db5b86ef13
[ddea253ff8]: https://github.com/NixOS/nix/commit/ddea253ff8
[e0204f8d46]: https://github.com/NixOS/nix/commit/e0204f8d46
[e0bd307802]: https://github.com/NixOS/nix/commit/e0bd307802
[e25fad691a]: https://github.com/NixOS/nix/commit/e25fad691a
[e34fe47d0c]: https://github.com/NixOS/nix/commit/e34fe47d0c
[e42401ee7b]: https://github.com/NixOS/nix/commit/e42401ee7b
[e5951a6b2f]: https://github.com/NixOS/nix/commit/e5951a6b2f
[eb3036da87]: https://github.com/NixOS/nix/commit/eb3036da87
[f3441e6122]: https://github.com/NixOS/nix/commit/f3441e6122
[f92c9a0ac5]: https://github.com/NixOS/nix/commit/f92c9a0ac5
[fe1f34fa60]: https://github.com/NixOS/nix/commit/fe1f34fa60
