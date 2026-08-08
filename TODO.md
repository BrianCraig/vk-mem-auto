## TODO

#### Image

| ID | Dependencies | Task | 
| -- | -- | -- |
| StagingUpload | None | Staging image upload test |
| AssertMove | None | Provide an assertion ensuring the resource has moved, test must fail if the resource did not move on defrag, this is to ensure that we are checking copy algorithms. It would be nice to have helpers that dependant of the tiling, they upload and check the data .  |
| AspectMaskCopy | AssertMove | On Image copy, they require a Mask, we should copy all the contents of the image, but that means that requires a table of the format to aspects, for example (RGBA8 => Mask::Color, D16_S8 => Mask::Depth | Mask::Stencil,  G8B8R8_3Plane => [Mask::P1, Mask::P2, Mask::P3]). Plane are special cases which require to have multiple SubresourceRange, and you cannot combine Mask::P1 | Mask::P2 like the others. |
| AspectMaskCopyTest | AssertMove | Testing suite for AssertMove, check every `format x (linear|optimal)` ensure its valid by checking against `vkGetPhysicalDeviceFormatProperties`, it should be available and accept `VK_FORMAT_FEATURE_TRANSFER_SRC_BIT` and `VK_FORMAT_FEATURE_TRANSFER_DST_BIT`. |

#### Buffer 

| ID | Dependencies | Task | 
| -- | -- | -- |

#### Code Quality

| ID | Dependencies | Task | 
| -- | -- | -- |
| Benchmarks | Images + Buffers | We should benchmark comple usage of the lib, probably based on a seeded PRNG, doing x amount of tests, with y amount of cycles of free+alloc and defrag. |
| AbsorbVMA | Benchmarks + Project | Transition fully to a Rust allocator inspired (or basically based of) current VMA. This is fairly complex, should be decomposed into smaller problems, but we need first to have a fully stable library, with integral testing suite + benchmarks |

#### Project

| ID | Dependencies | Task | 
| -- | -- | -- |
| Readme | None | Modify Readme to explain the intentions of this library, its fork, its inspiration, samples, and explanation of what is **not** the library, and the status. |
| NotCode | None | Remove what is not the library intended, exposed VMA_Allocator, pools, virtual memory... |
| Cleanup | None | `.vscode`, `CHANGES`, `CODE_OF_CONDUCT` |
| License | None | Determine if we are doing the correct licensing |
