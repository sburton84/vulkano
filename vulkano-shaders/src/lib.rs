// Copyright (c) 2016 The vulkano developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

extern crate glsl_to_spirv;

use std::env;
use std::fs;
use std::fs::File;
use std::io::Error as IoError;
use std::io::Read;
use std::io::Write;
use std::path::Path;

pub use parse::ParseError;
pub use glsl_to_spirv::ShaderType;

mod descriptor_sets;
mod enums;
mod parse;
mod structs;

pub fn build_glsl_shaders<'a, I>(shaders: I)
    where I: IntoIterator<Item = (&'a str, ShaderType)>
{
    let dest = env::var("OUT_DIR").unwrap();
    let dest = Path::new(&dest);

    for (shader, ty) in shaders {
        println!("cargo:rerun-if-changed={}", shader);
        let shader = Path::new(shader);

        let shader_content = {
            let mut s = String::new();
            File::open(shader).expect("failed to open shader").read_to_string(&mut s)
                              .expect("failed to read shader content");
            s
        };

        fs::create_dir_all(&dest.join("shaders").join(shader.parent().unwrap())).unwrap();
        let mut file_output = File::create(&dest.join("shaders").join(shader))
                                                        .expect("failed to open shader output");

        let content = glsl_to_spirv::compile(&shader_content, ty).unwrap();
        let output = reflect("Shader", content).unwrap();
        write!(file_output, "{}", output).unwrap();
    }
}

pub fn reflect<R>(name: &str, mut spirv: R) -> Result<String, Error>
    where R: Read
{
    let mut data = Vec::new();
    try!(spirv.read_to_end(&mut data));

    // now parsing the document
    let doc = try!(parse::parse_spirv(&data));

    // TODO: remove
    println!("{:#?}", doc);

    let mut output = String::new();

    {
        // contains the data that was passed as input to this function
        let spirv_data = data.iter().map(|&byte| byte.to_string())
                             .collect::<Vec<String>>()
                             .join(", ");

        // writing the header
        output.push_str(&format!(r#"
pub struct {name} {{
    shader: ::std::sync::Arc<::vulkano::shader::ShaderModule>,
}}

impl {name} {{
    /// Loads the shader in Vulkan as a `ShaderModule`.
    #[inline]
    pub fn load(device: &::std::sync::Arc<::vulkano::device::Device>)
                -> Result<{name}, ::vulkano::OomError>
    {{

        "#, name = name));

        // checking whether each required capability is supported by the vulkan implementation
        for i in doc.instructions.iter() {
            if let &parse::Instruction::Capability(ref cap) = i {
                if let Some(cap) = capability_name(cap) {
                    output.push_str(&format!(r#"
                        if !device.enabled_features().{cap} {{
                            panic!("capability {{:?}} not supported", "{cap}")  // FIXME: error
                            //return Err(CapabilityNotSupported);
                        }}"#, cap = cap));
                }
            }
        }

        // follow-up of the header
        output.push_str(&format!(r#"
        unsafe {{
            let data = [{spirv_data}];

            Ok({name} {{
                shader: try!(::vulkano::shader::ShaderModule::new(device, &data))
            }})
        }}
    }}

    /// Returns the module that was created.
    #[allow(dead_code)]
    #[inline]
    pub fn module(&self) -> &::std::sync::Arc<::vulkano::shader::ShaderModule> {{
        &self.shader
    }}
        "#, name = name, spirv_data = spirv_data));

        // writing one method for each entry point of this module
        for instruction in doc.instructions.iter() {
            if let &parse::Instruction::EntryPoint { .. } = instruction {
                output.push_str(&write_entry_point(&doc, instruction));
            }
        }

        // footer
        output.push_str(&format!(r#"
}}
        "#));

        // struct definitions
        output.push_str("pub mod ty {");
        output.push_str(&structs::write_structs(&doc));
        output.push_str("}");

        // descriptor sets
        output.push_str(&descriptor_sets::write_descriptor_sets(&doc));
    }

    Ok(output)
}

#[derive(Debug)]
pub enum Error {
    IoError(IoError),
    ParseError(ParseError),
}

impl From<IoError> for Error {
    #[inline]
    fn from(err: IoError) -> Error {
        Error::IoError(err)
    }
}

impl From<ParseError> for Error {
    #[inline]
    fn from(err: ParseError) -> Error {
        Error::ParseError(err)
    }
}

fn write_entry_point(doc: &parse::Spirv, instruction: &parse::Instruction) -> String {
    let (execution, ep_name, interface) = match instruction {
        &parse::Instruction::EntryPoint { ref execution, id, ref name, ref interface } => {
            (execution, name, interface)
        },
        _ => unreachable!()
    };

    let (ty, f_call) = match *execution {
        enums::ExecutionModel::ExecutionModelVertex => {
            let mut input_types = Vec::new();
            let mut attributes = Vec::new();

            // TODO: sort types by location

            for interface in interface.iter() {
                for i in doc.instructions.iter() {
                    match i {
                        &parse::Instruction::Variable { result_type_id, result_id,
                                    storage_class: enums::StorageClass::StorageClassInput, .. }
                                    if &result_id == interface =>
                        {
                            if is_builtin(doc, result_id) {
                                continue;
                            }

                            input_types.push(type_from_id(doc, result_type_id));
                            let name = name_from_id(doc, result_id);
                            let loc = match location_decoration(doc, result_id) {
                                Some(l) => l,
                                None => panic!("vertex attribute `{}` is missing a location", name)
                            };
                            attributes.push((loc, name));
                        },
                        _ => ()
                    }
                }
            }

            let input = {
                let input = input_types.join(", ");
                if input.is_empty() { input } else { input + "," }
            };

            let attributes = attributes.iter().map(|&(loc, ref name)| {
                format!("({}, ::std::borrow::Cow::Borrowed(\"{}\"))", loc, name)
            }).collect::<Vec<_>>().join(", ");

            let t = format!("::vulkano::shader::VertexShaderEntryPoint<({input}), Layout>",
                            input = input);
            let f = format!("vertex_shader_entry_point(::std::ffi::CStr::from_ptr(NAME.as_ptr() as *const _), Layout, vec![{}])", attributes);
            (t, f)
        },

        enums::ExecutionModel::ExecutionModelTessellationControl => {
            (format!("::vulkano::shader::TessControlShaderEntryPoint"), String::new())
        },

        enums::ExecutionModel::ExecutionModelTessellationEvaluation => {
            (format!("::vulkano::shader::TessEvaluationShaderEntryPoint"), String::new())
        },

        enums::ExecutionModel::ExecutionModelGeometry => {
            (format!("::vulkano::shader::GeometryShaderEntryPoint"), String::new())
        },

        enums::ExecutionModel::ExecutionModelFragment => {
            let mut output_types = Vec::new();

            for interface in interface.iter() {
                for i in doc.instructions.iter() {
                    match i {
                        &parse::Instruction::Variable { result_type_id, result_id,
                                    storage_class: enums::StorageClass::StorageClassOutput, .. }
                                    if &result_id == interface =>
                        {
                            output_types.push(type_from_id(doc, result_type_id));
                        },
                        _ => ()
                    }
                }
            }

            let output = {
                let output = output_types.join(", ");
                if output.is_empty() { output } else { output + "," }
            };

            let t = format!("::vulkano::shader::FragmentShaderEntryPoint<({output}), Layout>",
                            output = output);
            (t, format!("fragment_shader_entry_point(::std::ffi::CStr::from_ptr(NAME.as_ptr() as *const _), Layout)"))
        },

        enums::ExecutionModel::ExecutionModelGLCompute => {
            (format!("::vulkano::shader::ComputeShaderEntryPoint"), format!("compute_shader_entry_point"))
        },

        enums::ExecutionModel::ExecutionModelKernel => panic!("Kernels are not supported"),
    };

    format!(r#"
    /// Returns a logical struct describing the entry point named `{ep_name}`.
    #[inline]
    pub fn {ep_name}_entry_point(&self) -> {ty} {{
        unsafe {{
            #[allow(dead_code)]
            static NAME: [u8; {ep_name_lenp1}] = [{encoded_ep_name}, 0];     // "{ep_name}"
            self.shader.{f_call}
        }}
    }}
            "#, ep_name = ep_name, ep_name_lenp1 = ep_name.chars().count() + 1, ty = ty,
                encoded_ep_name = ep_name.chars().map(|c| (c as u32).to_string())
                                         .collect::<Vec<String>>().join(", "),
                f_call = f_call)
}

// TODO: struct definitions don't use this function, so irrelevant elements should be removed
fn type_from_id(doc: &parse::Spirv, searched: u32) -> String {
    for instruction in doc.instructions.iter() {
        match instruction {
            &parse::Instruction::TypeVoid { result_id } if result_id == searched => {
                return "()".to_owned()
            },
            &parse::Instruction::TypeBool { result_id } if result_id == searched => {
                return "bool".to_owned()
            },
            &parse::Instruction::TypeInt { result_id, width, signedness } if result_id == searched => {
                return "i32".to_owned()
            },
            &parse::Instruction::TypeFloat { result_id, width } if result_id == searched => {
                return "f32".to_owned()
            },
            &parse::Instruction::TypeVector { result_id, component_id, count } if result_id == searched => {
                let t = type_from_id(doc, component_id);
                return format!("[{}; {}]", t, count);
            },
            &parse::Instruction::TypeMatrix { result_id, column_type_id, column_count } if result_id == searched => {
                // FIXME: row-major or column-major
                let t = type_from_id(doc, column_type_id);
                return format!("[{}; {}]", t, column_count);
            },
            &parse::Instruction::TypeImage { result_id, sampled_type_id, ref dim, depth, arrayed, ms, sampled, ref format, ref access } if result_id == searched => {
                return format!("{}{}Texture{:?}{}{:?}",
                    if ms { "Multisample" } else { "" },
                    if depth == Some(true) { "Depth" } else { "" },
                    dim,
                    if arrayed { "Array" } else { "" },
                    format);
            },
            &parse::Instruction::TypeSampledImage { result_id, image_type_id } if result_id == searched => {
                return type_from_id(doc, image_type_id);
            },
            &parse::Instruction::TypeArray { result_id, type_id, length_id } if result_id == searched => {
                let t = type_from_id(doc, type_id);
                let len = doc.instructions.iter().filter_map(|e| {
                    match e { &parse::Instruction::Constant { result_id, ref data, .. } if result_id == length_id => Some(data.clone()), _ => None }
                }).next().expect("failed to find array length");
                let len = len.iter().rev().fold(0u64, |a, &b| (a << 32) | b as u64);
                return format!("[{}; {}]", t, len);       // FIXME:
            },
            &parse::Instruction::TypeRuntimeArray { result_id, type_id } if result_id == searched => {
                let t = type_from_id(doc, type_id);
                return format!("[{}]", t);
            },
            &parse::Instruction::TypeStruct { result_id, ref member_types } if result_id == searched => {
                let name = name_from_id(doc, result_id);
                return name;
            },
            &parse::Instruction::TypeOpaque { result_id, ref name } if result_id == searched => {
                return "<opaque>".to_owned();
            },
            &parse::Instruction::TypePointer { result_id, type_id, .. } if result_id == searched => {
                return type_from_id(doc, type_id);
            },
            _ => ()
        }
    }

    panic!("Type #{} not found", searched)
}

fn name_from_id(doc: &parse::Spirv, searched: u32) -> String {
    doc.instructions.iter().filter_map(|i| {
        if let &parse::Instruction::Name { target_id, ref name } = i {
            if target_id == searched {
                Some(name.clone())
            } else {
                None
            }
        } else {
            None
        }
    }).next().and_then(|n| if !n.is_empty() { Some(n) } else { None })
      .unwrap_or("__unnamed".to_owned())
}

fn member_name_from_id(doc: &parse::Spirv, searched: u32, searched_member: u32) -> String {
    doc.instructions.iter().filter_map(|i| {
        if let &parse::Instruction::MemberName { target_id, member, ref name } = i {
            if target_id == searched && member == searched_member {
                Some(name.clone())
            } else {
                None
            }
        } else {
            None
        }
    }).next().and_then(|n| if !n.is_empty() { Some(n) } else { None })
      .unwrap_or("__unnamed".to_owned())
}

fn location_decoration(doc: &parse::Spirv, searched: u32) -> Option<u32> {
    doc.instructions.iter().filter_map(|i| {
        if let &parse::Instruction::Decorate { target_id, decoration: enums::Decoration::DecorationLocation, ref params } = i {
            if target_id == searched {
                Some(params[0])
            } else {
                None
            }
        } else {
            None
        }
    }).next()
}

/// Returns true if a `BuiltIn` decorator is applied on an id.
fn is_builtin(doc: &parse::Spirv, id: u32) -> bool {
    for instruction in &doc.instructions {
        match *instruction {
            parse::Instruction::Decorate { target_id,
                                           decoration: enums::Decoration::DecorationBuiltIn,
                                           .. } if target_id == id =>
            {
                return true;
            },
            _ => ()
        }
    }

    false
}

/// Returns the name of the Vulkan something that corresponds to an `OpCapability`.
///
/// Returns `None` if irrelevant.
// TODO: this function is a draft, as the actual names may not be the same
fn capability_name(cap: &enums::Capability) -> Option<&'static str> {
    match *cap {
        enums::Capability::CapabilityMatrix => None,        // always supported
        enums::Capability::CapabilityShader => None,        // always supported
        enums::Capability::CapabilityGeometry => Some("geometry_shader"),
        enums::Capability::CapabilityTessellation => Some("tessellation_shader"),
        enums::Capability::CapabilityAddresses => panic!(), // not supported
        enums::Capability::CapabilityLinkage => panic!(),   // not supported
        enums::Capability::CapabilityKernel => panic!(),    // not supported
        enums::Capability::CapabilityVector16 => panic!(),  // not supported
        enums::Capability::CapabilityFloat16Buffer => panic!(), // not supported
        enums::Capability::CapabilityFloat16 => panic!(),   // not supported
        enums::Capability::CapabilityFloat64 => Some("shader_f3264"),
        enums::Capability::CapabilityInt64 => Some("shader_int64"),
        enums::Capability::CapabilityInt64Atomics => panic!(),  // not supported
        enums::Capability::CapabilityImageBasic => panic!(),    // not supported
        enums::Capability::CapabilityImageReadWrite => panic!(),    // not supported
        enums::Capability::CapabilityImageMipmap => panic!(),   // not supported
        enums::Capability::CapabilityPipes => panic!(), // not supported
        enums::Capability::CapabilityGroups => panic!(),    // not supported
        enums::Capability::CapabilityDeviceEnqueue => panic!(), // not supported
        enums::Capability::CapabilityLiteralSampler => panic!(),    // not supported
        enums::Capability::CapabilityAtomicStorage => panic!(), // not supported
        enums::Capability::CapabilityInt16 => Some("shader_int16"),
        enums::Capability::CapabilityTessellationPointSize => Some("shader_tessellation_and_geometry_point_size"),
        enums::Capability::CapabilityGeometryPointSize => Some("shader_tessellation_and_geometry_point_size"),
        enums::Capability::CapabilityImageGatherExtended => Some("shader_image_gather_extended"),
        enums::Capability::CapabilityStorageImageMultisample => Some("shader_storage_image_multisample"),
        enums::Capability::CapabilityUniformBufferArrayDynamicIndexing => Some("shader_uniform_buffer_array_dynamic_indexing"),
        enums::Capability::CapabilitySampledImageArrayDynamicIndexing => Some("shader_sampled_image_array_dynamic_indexing"),
        enums::Capability::CapabilityStorageBufferArrayDynamicIndexing => Some("shader_storage_buffer_array_dynamic_indexing"),
        enums::Capability::CapabilityStorageImageArrayDynamicIndexing => Some("shader_storage_image_array_dynamic_indexing"),
        enums::Capability::CapabilityClipDistance => Some("shader_clip_distance"),
        enums::Capability::CapabilityCullDistance => Some("shader_cull_distance"),
        enums::Capability::CapabilityImageCubeArray => Some("image_cube_array"),
        enums::Capability::CapabilitySampleRateShading => Some("sample_rate_shading"),
        enums::Capability::CapabilityImageRect => panic!(), // not supported
        enums::Capability::CapabilitySampledRect => panic!(),   // not supported
        enums::Capability::CapabilityGenericPointer => panic!(),    // not supported
        enums::Capability::CapabilityInt8 => panic!(),  // not supported
        enums::Capability::CapabilityInputAttachment => None,       // always supported
        enums::Capability::CapabilitySparseResidency => Some("shader_resource_residency"),
        enums::Capability::CapabilityMinLod => Some("shader_resource_min_lod"),
        enums::Capability::CapabilitySampled1D => None,        // always supported
        enums::Capability::CapabilityImage1D => None,        // always supported
        enums::Capability::CapabilitySampledCubeArray => Some("image_cube_array"),
        enums::Capability::CapabilitySampledBuffer => None,         // always supported
        enums::Capability::CapabilityImageBuffer => None,        // always supported
        enums::Capability::CapabilityImageMSArray => Some("shader_storage_image_multisample"),
        enums::Capability::CapabilityStorageImageExtendedFormats => Some("shader_storage_image_extended_formats"),
        enums::Capability::CapabilityImageQuery => None,        // always supported
        enums::Capability::CapabilityDerivativeControl => None,        // always supported
        enums::Capability::CapabilityInterpolationFunction => Some("sample_rate_shading"),
        enums::Capability::CapabilityTransformFeedback => panic!(), // not supported
        enums::Capability::CapabilityGeometryStreams => panic!(),   // not supported
        enums::Capability::CapabilityStorageImageReadWithoutFormat => Some("shader_storage_image_read_without_format"),
        enums::Capability::CapabilityStorageImageWriteWithoutFormat => Some("shader_storage_image_write_without_format"),
        enums::Capability::CapabilityMultiViewport => Some("multi_viewport"),
    }
}
