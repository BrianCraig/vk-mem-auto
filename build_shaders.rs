fn compile_shader(
    wgsl_source: &str,
    stage: naga::ShaderStage,
    entry_point: &str,
    output: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    let module = naga::front::wgsl::parse_str(wgsl_source)?;

    let module_info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    )
    .subgroup_stages(naga::valid::ShaderStages::all())
    .subgroup_operations(naga::valid::SubgroupOperationSet::all())
    .validate(&module)?;

    let spv = naga::back::spv::write_vec(
        &module,
        &module_info,
        &naga::back::spv::Options {
            lang_version: (1, 3),
            ..Default::default()
        },
        Some(&naga::back::spv::PipelineOptions {
            shader_stage: stage,
            entry_point: entry_point.into(),
        }),
    )?;

    std::fs::write(output, bytemuck::cast_slice::<u32, u8>(&spv))?;
    Ok(())
}

pub fn build() -> Result<(), Box<dyn std::error::Error>> {
    compile_shader(
        r#"
const POSITIONS = array<vec2<f32>, 6>(
    vec2(-1.0, -1.0),
    vec2( 1.0, -1.0),
    vec2(-1.0,  1.0),

    vec2(-1.0,  1.0),
    vec2( 1.0, -1.0),
    vec2( 1.0,  1.0),
);

@vertex
fn main_vs(
    @builtin(vertex_index) i: u32
) -> @builtin(position) vec4<f32> {
    return vec4(POSITIONS[i], 0.0, 1.0);
}
"#,
        naga::ShaderStage::Vertex,
        "main_vs",
        "shaders/vertex_fullscreen.spv",
    )?;

    compile_shader(
        r#"
@fragment
fn main_fs() -> @location(0) vec4<f32> {
    return vec4(1.0, 1.0, 1.0, 1.0);
}
"#,
        naga::ShaderStage::Fragment,
        "main_fs",
        "shaders/fragment_white.spv",
    )?;

    Ok(())
}
