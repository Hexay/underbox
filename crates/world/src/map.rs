//! A compiled map as a whole: `<folder>/world.vwrld_c` names the world nodes and entity lumps,
//! which load after it. Files come through a lookup, so a pak, a folder or both can back it.

use crate::Error;
use crate::entities::{self, Entity, EntityLump};
use crate::node::{self, SceneObject, WorldNode};
use crate::resource;
use crate::values::{array, field_str};
use std::collections::HashSet;

#[derive(Debug, Default)]
pub struct Map {
    /// The map's folder, `maps/<name>`.
    pub folder: String,
    pub nodes: Vec<WorldNode>,
    /// Entity lumps by resource path, roots first, children after their parents.
    pub lumps: Vec<(String, EntityLump)>,
    /// Files the world names that are missing or unreadable; the rest still loads.
    pub warnings: Vec<String>,
}

/// The compiled file for a resource name (`x.vents` → `x.vents_c`).
fn compiled(path: &str) -> String {
    if path.ends_with("_c") { path.to_owned() } else { format!("{path}_c") }
}

fn stem(path: &str) -> &str {
    let file = path.rsplit('/').next().unwrap_or(path);
    file.split('.').next().unwrap_or(file)
}

impl Map {
    /// Load the map in `folder` (`maps/foo`), reading files through `read`.
    pub fn load(folder: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Result<Map, Error> {
        let world_path = format!("{folder}/world.vwrld_c");
        let world = resource::data(&read(&world_path).ok_or(Error::Missing(world_path))?)?;
        let o = world.as_object().ok_or_else(|| Error::Format("world DATA isn't an object".into()))?;
        let mut map = Map { folder: folder.to_owned(), ..Map::default() };
        for n in array(o, "m_worldNodes").iter().filter_map(|n| n.as_object()) {
            let path = format!("{}.vwnod_c", field_str(n, "m_worldNodePrefix"));
            match map.resource(&path, read).map(|d| node::parse(&d)) {
                Some(Ok(node)) => map.nodes.push(node),
                Some(Err(e)) => map.warnings.push(format!("{path}: {e}")),
                None => {}
            }
        }
        let mut seen = HashSet::new();
        let mut pending: Vec<String> = array(o, "m_entityLumps").iter().filter_map(|v| v.as_str()).map(compiled).collect();
        pending.reverse();
        while let Some(path) = pending.pop() {
            if !seen.insert(path.to_ascii_lowercase()) {
                continue;
            }
            match map.resource(&path, read).map(|d| entities::parse(&d)) {
                Some(Ok(lump)) => {
                    pending.extend(lump.children.iter().rev().map(|c| compiled(c)));
                    map.lumps.push((path, lump));
                }
                Some(Err(e)) => map.warnings.push(format!("{path}: {e}")),
                None => {}
            }
        }
        Ok(map)
    }

    /// A resource's decoded DATA; `None` (with a warning) when it's missing or unreadable.
    fn resource(&mut self, path: &str, read: &dyn Fn(&str) -> Option<Vec<u8>>) -> Option<kv::Value> {
        let Some(bytes) = read(path) else {
            self.warnings.push(format!("{path} is missing"));
            return None;
        };
        resource::data(&bytes).inspect_err(|e| self.warnings.push(format!("{path}: {e}"))).ok()
    }

    pub fn objects(&self) -> impl Iterator<Item = &SceneObject> {
        self.nodes.iter().flat_map(|n| n.objects.iter())
    }

    /// The entities of lump `name` (its `m_name`, or its file name without extension), followed
    /// by those of its child lumps, depth first.
    pub fn entities(&self, name: &str) -> Vec<&Entity> {
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        if let Some(i) = self.lumps.iter().position(|(path, l)| l.name.eq_ignore_ascii_case(name) || stem(path).eq_ignore_ascii_case(name)) {
            self.collect(i, &mut out, &mut seen);
        }
        out
    }

    fn collect<'a>(&'a self, i: usize, out: &mut Vec<&'a Entity>, seen: &mut HashSet<usize>) {
        if !seen.insert(i) {
            return;
        }
        let (_, lump) = &self.lumps[i];
        out.extend(lump.entities.iter());
        for child in &lump.children {
            let child = compiled(child);
            if let Some(j) = self.lumps.iter().position(|(p, _)| p.eq_ignore_ascii_case(&child)) {
                self.collect(j, out, seen);
            }
        }
    }
}
