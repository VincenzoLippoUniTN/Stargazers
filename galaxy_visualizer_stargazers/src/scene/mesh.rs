use bevy::prelude::*;
use bevy::render::mesh::{Indices, PrimitiveTopology};
use std::f32::consts::TAU;

/// A closed elliptical orbit drawn as a line strip.
pub fn orbit(radius: f32, eccentricity: f32, tilt: Quat) -> Mesh {
    let segments = 200;
    let mut verts: Vec<[f32; 3]> = Vec::with_capacity(segments + 1);

    for i in 0..=segments {
        let t = (i as f32 / segments as f32) * TAU;
        let pos = tilt * Vec3::new(radius * t.cos(), 0.0, radius * eccentricity * t.sin());
        verts.push(pos.into());
    }

    // Index every vertex, including the duplicated start point, so the loop closes.
    let indices: Vec<u32> = (0..=segments as u32).collect();
    let normals = vec![[0.0, 1.0, 0.0]; verts.len()];
    let uvs = vec![[0.0, 0.0]; verts.len()];

    Mesh::new(PrimitiveTopology::LineStrip, default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, verts)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, normals)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uvs)
        .with_inserted_indices(Indices::U32(indices))
}

/// A flat planetary ring, built as a triangle list of quads.
pub fn ring(inner: f32, outer: f32) -> Mesh {
    let segments = 64;
    let mut pos = Vec::new();
    let mut norm = Vec::new();
    let mut uv = Vec::new();
    let mut idx = Vec::new();

    for i in 0..=segments {
        let a = (i as f32 / segments as f32) * TAU;
        let (s, c) = (a.sin(), a.cos());

        pos.push([inner * c, 0.0, inner * s]);
        pos.push([outer * c, 0.0, outer * s]);
        norm.push([0.0, 1.0, 0.0]);
        norm.push([0.0, 1.0, 0.0]);
        uv.push([i as f32 / segments as f32, 0.0]);
        uv.push([i as f32 / segments as f32, 1.0]);

        if i < segments {
            let b = (i * 2) as u32;
            idx.extend([b, b + 2, b + 1, b + 1, b + 2, b + 3]);
        }
    }

    Mesh::new(PrimitiveTopology::TriangleList, default())
        .with_inserted_attribute(Mesh::ATTRIBUTE_POSITION, pos)
        .with_inserted_attribute(Mesh::ATTRIBUTE_NORMAL, norm)
        .with_inserted_attribute(Mesh::ATTRIBUTE_UV_0, uv)
        .with_inserted_indices(Indices::U32(idx))
}
