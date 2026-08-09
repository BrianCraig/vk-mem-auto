## TODO

#### Image

| ID | Dependencies | Task | 
| -- | -- | -- |
| AssertMove | None | Provide an assertion ensuring the resource has moved, test must fail if the resource did not move on defrag, this is to ensure that we are checking copy algorithms. It would be nice to have helpers that dependant of the tiling, they upload and check the data. |
| AspectMaskCopy | AssertMove | On Image copy, they require a Mask, we should copy all the contents of the image, but that means that requires a table of the format to aspects, for example (RGBA8 => Mask::Color, D16_S8 => Mask::Depth \| Mask::Stencil,  G8B8R8_3Plane => [Mask::P1, Mask::P2, Mask::P3]). Plane are special cases which require to have multiple SubresourceRange, and you cannot combine Mask::P1 \| Mask::P2 like the others. |
| AspectMaskCopyTest | AssertMove | Testing suite for AssertMove, check every `format` on `linear` tiling, ensure its valid by checking against `vkGetPhysicalDeviceFormatProperties`, it should be available and accept `VK_FORMAT_FEATURE_TRANSFER_SRC_BIT` and `VK_FORMAT_FEATURE_TRANSFER_DST_BIT`. |
| MipsCopy | AssertMove | All mips should be copied (only `linear`). |
| MipsCopyTest | AssertMove | Test for MipsCopy (only `linear`). |
| LayersCopy | AssertMove | All layers should be copied (only `linear`). |
| LayersCopyTest | AssertMove | Test for LayersCopy (only `linear`). |
| ZDimCopy | AssertMove | All Z Dimensions of a 3D Image should be copied (only `linear`). |
| ZDimCopyTest | AssertMove | Test for ZDimCopy (only `linear`). |
| OptimalCopyTest | \*Copy\[Test\] | We been ignoring `optimal` images because asserting that the image has been fully copied (aspect + mips + layers + zdim) requires a staging buffer and a vkImgCopy which ensures that all these subregions have been replicated. Since the code required for this is usually gotten from the knowledge of copy features, its obvious to do this after the full scope of linear copy is done. Ensure `optimal` images are correctly uploaded and asserted, on `DEVICE_LOCAL` memory, using a staging image. |

#### Buffer 

| ID | Dependencies | Task | 
| -- | -- | -- |

#### Code Quality

| ID | Dependencies | Task | 
| -- | -- | -- |
| AllocatorAllocParams | None | Allocator (handle) could be just `pub fn allocate(&self, resource: impl TryInto<ResourceInfo, Error = ResourceInfoError>, config: impl Into<AllocationConfig>,) -> VkResult<ManagedAllocationHandle>`, and there should be no diff of type of resource |
| Benchmarks | Images + Buffers | We should benchmark complex usage of the lib, probably based on a seeded PRNG, doing x amount of tests, with y amount of cycles of free+alloc and defrag. |
| PerformanceLazilyBind | Benchmarks | Since we are returning a Handle, there is no underlying need to bind the memory immediately, just when the user asks for the vk resource associated, or maps the resource. Allocating when needed can cause a better allocation spot, or even never allocate. This performance is kind of too questionable but may be good to take a look once benchmarks are available |
| AbsorbVMA | Benchmarks + Project | Transition fully to a Rust allocator inspired (or basically based of) current VMA. This is fairly complex, should be decomposed into smaller problems, but we need first to have a fully stable library, with integral testing suite + benchmarks |

#### Project

| ID | Dependencies | Task | 
| -- | -- | -- |
| Readme | None | Modify Readme to explain the intentions of this library, its fork, its inspiration, samples, and explanation of what is **not** the library, and the status. |
| NotCode | None | Remove what is not the library intended, exposed VMA_Allocator, pools, virtual memory... |
| Cleanup | None | `.vscode`, `CHANGES`, `CODE_OF_CONDUCT` |
| License | None | Determine if we are doing the correct licensing |
