// Copyright (c) 2016 The vulkano developers
// Licensed under the Apache License, Version 2.0
// <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT
// license <LICENSE-MIT or http://opensource.org/licenses/MIT>,
// at your option. All files in the project carrying such
// notice may not be copied, modified, or distributed except
// according to those terms.

use std::sync::Arc;

use buffer::Buffer;
use descriptor_set::AbstractDescriptorSet;
use descriptor_set::AbstractDescriptorSetLayout;
use image::ImageView;
use image::Layout as ImageLayout;
use sampler::Sampler;

use vk;

/// Types that describe the layout of a pipeline (descriptor sets and push constants).
pub unsafe trait Layout {
    /// Represents a collection of `DescriptorSet` structs. A parameter of this type must be
    /// passed when you add a draw command to a command buffer that uses this layout.
    type DescriptorSets;

    /// Represents a collection of `DescriptorSetLayout` structs. A parameter of this type must
    /// be passed when creating a `PipelineLayout` struct.
    type DescriptorSetLayouts;

    /// Not yet implemented. Useless for now.
    type PushConstants;

    /// Turns the `DescriptorSets` associated type into something vulkano can understand.
    fn decode_descriptor_sets(&self, Self::DescriptorSets) -> Vec<Arc<AbstractDescriptorSet>>;  // TODO: vec is slow

    /// Turns the `DescriptorSetLayouts` associated type into something vulkano can understand.
    fn decode_descriptor_set_layouts(&self, Self::DescriptorSetLayouts)
                                     -> Vec<Arc<AbstractDescriptorSetLayout>>;  // TODO: vec is slow
}

/// Extension for `Layout`.
pub unsafe trait LayoutPossibleSuperset<Other>: Layout where Other: Layout {
    /// Returns true if `self` is a superset of `Other`. That is, all the descriptors in `Other`
    /// are also in `self` and have an identical definition.
    fn is_superset_of(&self, &Other) -> bool;
}

// CRITICAL FIXME: temporary hack
unsafe impl<T, U> LayoutPossibleSuperset<U> for T where T: Layout, U: Layout {
    #[inline]
    fn is_superset_of(&self, _: &U) -> bool { true }
}

/// Types that describe a single descriptor set.
pub unsafe trait SetLayout {
    /// Returns the list of descriptors contained in this set.
    fn descriptors(&self) -> Vec<DescriptorDesc>;       // TODO: better perfs
}

/// Extension for the `SetLayout` trait.
pub unsafe trait SetLayoutWrite<Data>: SetLayout {
    /// Turns the data into something vulkano can understand.
    fn decode(&self, Data) -> Vec<DescriptorWrite>;        // TODO: better perfs
}

/// Extension for the `SetLayout` trait.
pub unsafe trait SetLayoutInit<Data>: SetLayout {
    /// Turns the data into something vulkano can understand.
    fn decode(&self, Data) -> Vec<DescriptorWrite>;        // TODO: better perfs
}

/// Extension for `SetLayout`.
pub unsafe trait SetLayoutPossibleSuperset<Other>: SetLayout where Other: SetLayout {
    /// Returns true if `self` is a superset of `Other`. That is, all the descriptors in `Other`
    /// are also in `self` and have an identical definition.
    fn is_superset_of(&self, &Other) -> bool;
}

// FIXME: shoud allow multiple array binds at once
pub struct DescriptorWrite {
    pub binding: u32,
    pub array_element: u32,
    pub content: DescriptorBind,
}

// FIXME: incomplete
#[derive(Clone)]        // TODO: Debug
pub enum DescriptorBind {
    StorageImage(Arc<ImageView>, ImageLayout),
    Sampler(Arc<Sampler>),
    SampledImage(Arc<ImageView>, ImageLayout),
    CombinedImageSampler(Arc<Sampler>, Arc<ImageView>, ImageLayout),
    //UniformTexelBuffer(Arc<Buffer>),      // FIXME: requires buffer views
    //StorageTexelBuffer(Arc<Buffer>),      // FIXME: requires buffer views
    UniformBuffer { buffer: Arc<Buffer>, offset: usize, size: usize },
    StorageBuffer { buffer: Arc<Buffer>, offset: usize, size: usize },
    DynamicUniformBuffer { buffer: Arc<Buffer>, offset: usize, size: usize },
    DynamicStorageBuffer { buffer: Arc<Buffer>, offset: usize, size: usize },
    InputAttachment(Arc<ImageView>, ImageLayout),
}

impl DescriptorBind {
    /// Returns the type corresponding to this bind.
    #[inline]
    pub fn ty(&self) -> DescriptorType {
        match *self {
            DescriptorBind::Sampler(_) => DescriptorType::Sampler,
            DescriptorBind::CombinedImageSampler(_, _, _) => DescriptorType::CombinedImageSampler,
            DescriptorBind::SampledImage(_, _) => DescriptorType::SampledImage,
            DescriptorBind::StorageImage(_, _) => DescriptorType::StorageImage,
            //DescriptorBind::UniformTexelBuffer(_) => DescriptorType::UniformTexelBuffer,
            //DescriptorBind::StorageTexelBuffer(_) => DescriptorType::StorageTexelBuffer,
            DescriptorBind::UniformBuffer { .. } => DescriptorType::UniformBuffer,
            DescriptorBind::StorageBuffer { .. } => DescriptorType::StorageBuffer,
            DescriptorBind::DynamicUniformBuffer { .. } => DescriptorType::UniformBufferDynamic,
            DescriptorBind::DynamicStorageBuffer { .. } => DescriptorType::StorageBufferDynamic,
            DescriptorBind::InputAttachment(_, _) => DescriptorType::InputAttachment,
        }
    }
}

/// Describes a single descriptor.
#[derive(Debug, Copy, Clone)]
pub struct DescriptorDesc {
    /// Offset of the binding within the descriptor.
    pub binding: u32,

    /// What kind of resource can later be bind to this descriptor.
    pub ty: DescriptorType,

    /// How many array elements this descriptor is made of.
    pub array_count: u32,

    /// Which shader stages are going to access this descriptor.
    pub stages: ShaderStages,
}

/// Describes what kind of resource may later be bound to a descriptor.
// FIXME: add immutable sampler when relevant
#[derive(Debug, Copy, Clone)]
#[repr(u32)]
pub enum DescriptorType {
    Sampler = vk::DESCRIPTOR_TYPE_SAMPLER,
    CombinedImageSampler = vk::DESCRIPTOR_TYPE_COMBINED_IMAGE_SAMPLER,
    SampledImage = vk::DESCRIPTOR_TYPE_SAMPLED_IMAGE,
    StorageImage = vk::DESCRIPTOR_TYPE_STORAGE_IMAGE,
    UniformTexelBuffer = vk::DESCRIPTOR_TYPE_UNIFORM_TEXEL_BUFFER,
    StorageTexelBuffer = vk::DESCRIPTOR_TYPE_STORAGE_TEXEL_BUFFER,
    UniformBuffer = vk::DESCRIPTOR_TYPE_UNIFORM_BUFFER,
    StorageBuffer = vk::DESCRIPTOR_TYPE_STORAGE_BUFFER,
    UniformBufferDynamic = vk::DESCRIPTOR_TYPE_UNIFORM_BUFFER_DYNAMIC,
    StorageBufferDynamic = vk::DESCRIPTOR_TYPE_STORAGE_BUFFER_DYNAMIC,
    InputAttachment = vk::DESCRIPTOR_TYPE_INPUT_ATTACHMENT,
}

impl DescriptorType {
    /// Turns the `DescriptorType` into the corresponding Vulkan constant.
    // this function exists because when immutable samplers are added, it will no longer be possible to do `as u32`
    // TODO: hacky
    #[inline]
    #[doc(hidden)]
    pub fn vk_enum(&self) -> u32 {
        *self as u32
    }
}

/// Describes which shader stages have access to a descriptor.
#[derive(Debug, Copy, Clone)]
pub struct ShaderStages {
    /// `True` means that the descriptor will be used by the vertex shader.
    pub vertex: bool,
    /// `True` means that the descriptor will be used by the tessellation control shader.
    pub tessellation_control: bool,
    /// `True` means that the descriptor will be used by the tessellation evaluation shader.
    pub tessellation_evaluation: bool,
    /// `True` means that the descriptor will be used by the geometry shader.
    pub geometry: bool,
    /// `True` means that the descriptor will be used by the fragment shader.
    pub fragment: bool,
    /// `True` means that the descriptor will be used by the compute shader.
    pub compute: bool,
}

impl ShaderStages {
    /// Creates a `ShaderStages` struct will all stages set to `true`.
    #[inline]
    pub fn all() -> ShaderStages {
        ShaderStages {
            vertex: true,
            tessellation_control: true,
            tessellation_evaluation: true,
            geometry: true,
            fragment: true,
            compute: true,
        }
    }

    /// Creates a `ShaderStages` struct will all graphics stages set to `true`.
    #[inline]
    pub fn all_graphics() -> ShaderStages {
        ShaderStages {
            vertex: true,
            tessellation_control: true,
            tessellation_evaluation: true,
            geometry: true,
            fragment: true,
            compute: false,
        }
    }

    /// Creates a `ShaderStages` struct will the compute stage set to `true`.
    #[inline]
    pub fn compute() -> ShaderStages {
        ShaderStages {
            vertex: false,
            tessellation_control: false,
            tessellation_evaluation: false,
            geometry: false,
            fragment: false,
            compute: true,
        }
    }
}

#[doc(hidden)]
impl Into<vk::ShaderStageFlags> for ShaderStages {
    #[inline]
    fn into(self) -> vk::ShaderStageFlags {
        let mut result = 0;
        if self.vertex { result |= vk::SHADER_STAGE_VERTEX_BIT; }
        if self.tessellation_control { result |= vk::SHADER_STAGE_TESSELLATION_CONTROL_BIT; }
        if self.tessellation_evaluation { result |= vk::SHADER_STAGE_TESSELLATION_EVALUATION_BIT; }
        if self.geometry { result |= vk::SHADER_STAGE_GEOMETRY_BIT; }
        if self.fragment { result |= vk::SHADER_STAGE_FRAGMENT_BIT; }
        if self.compute { result |= vk::SHADER_STAGE_COMPUTE_BIT; }
        result
    }
}

#[macro_export]
macro_rules! pipeline_from_sets {
    ($($set:ty),*) => {
        use std::sync::Arc;
        use $crate::descriptor_set::AbstractDescriptorSet;
        use $crate::descriptor_set::AbstractDescriptorSetLayout;
        use $crate::descriptor_set::DescriptorSet;
        use $crate::descriptor_set::DescriptorSetLayout;
        use $crate::descriptor_set::DescriptorSetsCollection;

        pub struct Layout;

        pub type DescriptorSets = ($(Arc<DescriptorSet<$set>>,)*);
        pub type DescriptorSetLayouts = ($(Arc<DescriptorSetLayout<$set>>,)*);

        unsafe impl $crate::descriptor_set::Layout for Layout {
            type DescriptorSets = DescriptorSets;
            type DescriptorSetLayouts = DescriptorSetLayouts;
            type PushConstants = ();

            fn decode_descriptor_sets(&self, sets: DescriptorSets) -> Vec<Arc<AbstractDescriptorSet>> {
                DescriptorSetsCollection::list(&sets).collect()
            }

            /// Turns the `DescriptorSetLayouts` associated type into something vulkano can understand.
            fn decode_descriptor_set_layouts(&self, sets: DescriptorSetLayouts)
                                             -> Vec<Arc<AbstractDescriptorSetLayout>>
            {
                // FIXME:
                vec![sets.0.clone() as Arc<_>]
            }
        }
    };
}
