# Third-Party Notices & Asset Attribution

This document records third-party code and assets bundled with or utilized by Muvrix, along with their associated licenses, version identifiers, and cryptographic integrity hashes.

---

## 1. Anime4K GLSL Shaders

- **Component:** Anime4K GLSL Shaders (mpv user shaders)
- **Upstream Author:** bloc97 (<https://github.com/bloc97/Anime4K>)
- **Version:** v4.0.1
- **License:** MIT License
- **Asset Directory:** `assets/shaders/Anime4K/upstream/`
- **Reference Implementation:** mpvEx Android Client (`/home/itsyamin/Codes/mpvEx`)

### 1.1 License Text

```text
MIT License

Copyright (c) 2019-2021 bloc97
All rights reserved.

Permission is hereby granted, free of charge, to any person obtaining a copy
of this software and associated documentation files (the "Software"), to deal
in the Software without restriction, including without limitation the rights
to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
copies of the Software, and to permit persons to whom the Software is
furnished to do so, subject to the following conditions:

The above copyright notice and this permission notice shall be included in all
copies or substantial portions of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
SOFTWARE.
```

### 1.2 Asset Inventory & SHA-256 Checksums

| File Name | Size (Bytes) | SHA-256 Checksum | Category |
| :--- | :--- | :--- | :--- |
| `Anime4K_AutoDownscalePre_x2.glsl` | 1,560 | `8c58291740146bd766a4d73f132775a797fe80f7d07919b5d767e27a5dc85656` | Downscaling Pre-pass |
| `Anime4K_Clamp_Highlights.glsl` | 2,795 | `a2a9bf7fbc1d75d09660ca2e701e4d7fb0cf5457b94da47e1825032fa2b3671a` | Highlight Clamping & De-ringing |
| `Anime4K_Restore_CNN_S.glsl` | 17,136 | `97c24dc370ab300c108bfaa09db7f175aeff343674842c299cf3940a3d330427` | Line Restoration (Fast / Small) |
| `Anime4K_Restore_CNN_M.glsl` | 35,916 | `67ea3ed26539e8de3b7d307688535d2ff17e8d147e11dda0247da7770dbecf41` | Line Restoration (Balanced / Medium) |
| `Anime4K_Restore_CNN_L.glsl` | 69,921 | `d6efe215e6ee8af1ec560478a91afc1df83fac4ba43b2c806ee61ca2267ed674` | Line Restoration (High / Large) |
| `Anime4K_Restore_CNN_Pass2_S.glsl` | 17,136 | `97c24dc370ab300c108bfaa09db7f175aeff343674842c299cf3940a3d330427` | Line Restoration Pass 2 (Fast / Small) |
| `Anime4K_Restore_CNN_Pass2_M.glsl` | 35,916 | `67ea3ed26539e8de3b7d307688535d2ff17e8d147e11dda0247da7770dbecf41` | Line Restoration Pass 2 (Balanced / Medium) |
| `Anime4K_Restore_CNN_Pass2_L.glsl` | 69,921 | `d6efe215e6ee8af1ec560478a91afc1df83fac4ba43b2c806ee61ca2267ed674` | Line Restoration Pass 2 (High / Large) |
| `Anime4K_Restore_CNN_Soft_S.glsl` | 17,198 | `9f6867f2ef42786729522d86fe24147cb4ea145418e3974df70496acf52dc392` | Soft Line Restoration (Fast / Small) |
| `Anime4K_Restore_CNN_Soft_M.glsl` | 36,016 | `a78a2c76898e08e09e442a9628c64208c26e8e15789649b8755223f009794c02` | Soft Line Restoration (Balanced / Medium) |
| `Anime4K_Restore_CNN_Soft_L.glsl` | 70,020 | `8f2a5c73b526c6e4c67bce0366f7dcd7410bfa4a31e718240d86f53660788e63` | Soft Line Restoration (High / Large) |
| `Anime4K_Restore_CNN_Soft_Pass2_S.glsl` | 17,198 | `9f6867f2ef42786729522d86fe24147cb4ea145418e3974df70496acf52dc392` | Soft Line Restoration Pass 2 (Fast / Small) |
| `Anime4K_Restore_CNN_Soft_Pass2_M.glsl` | 36,016 | `a78a2c76898e08e09e442a9628c64208c26e8e15789649b8755223f009794c02` | Soft Line Restoration Pass 2 (Balanced / Medium) |
| `Anime4K_Restore_CNN_Soft_Pass2_L.glsl` | 70,020 | `8f2a5c73b526c6e4c67bce0366f7dcd7410bfa4a31e718240d86f53660788e63` | Soft Line Restoration Pass 2 (High / Large) |
| `Anime4K_Upscale_CNN_x2_S.glsl` | 18,638 | `4c53ec2e287908f7ee7bcb266b0170421626d663576468b7d7dafc62962649a4` | CNN 2x Upscaling (Fast / Small) |
| `Anime4K_Upscale_CNN_x2_M.glsl` | 37,685 | `716e02098a68f0d648761f2b96b4dd139e1cb09b174bb369fca3aa34328fff7e` | CNN 2x Upscaling (Balanced / Medium) |
| `Anime4K_Upscale_CNN_x2_L.glsl` | 73,443 | `db1fedf7be82f6fd9034e6bf39b64daf2b7576988bb584ec38f24f5236b1cd97` | CNN 2x Upscaling (High / Large) |
| `Anime4K_Upscale_CNN_x2_Pass2_S.glsl` | 18,638 | `4c53ec2e287908f7ee7bcb266b0170421626d663576468b7d7dafc62962649a4` | CNN 2x Upscaling Pass 2 (Fast / Small) |
| `Anime4K_Upscale_CNN_x2_Pass2_M.glsl` | 37,685 | `716e02098a68f0d648761f2b96b4dd139e1cb09b174bb369fca3aa34328fff7e` | CNN 2x Upscaling Pass 2 (Balanced / Medium) |
| `Anime4K_Upscale_CNN_x2_Pass2_L.glsl` | 73,443 | `db1fedf7be82f6fd9034e6bf39b64daf2b7576988bb584ec38f24f5236b1cd97` | CNN 2x Upscaling Pass 2 (High / Large) |
| `Anime4K_Upscale_Denoise_CNN_x2_S.glsl` | 18,667 | `1a45ad3aa20d8368399f2fd46791deed957c2fd0a4afd131cc92516827abab93` | Denoise & 2x Upscale (Fast / Small) |
| `Anime4K_Upscale_Denoise_CNN_x2_M.glsl` | 37,714 | `8c72b042e2301fe66a45c3089720459148e2504cd72af16f9c0d5017ff14181e` | Denoise & 2x Upscale (Balanced / Medium) |
| `Anime4K_Upscale_Denoise_CNN_x2_L.glsl` | 73,584 | `6cc4604c9544fd4fd9e3a75fd797511bf5fe1e626e9e1dbcee03302823a63207` | Denoise & 2x Upscale (High / Large) |
