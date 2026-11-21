use crate::error::{Result, WinedroidError};
use std::collections::{BTreeMap, BTreeSet};

const RES_STRING_POOL_TYPE: u16 = 0x0001;
const RES_TABLE_TYPE: u16 = 0x0002;
const RES_TABLE_PACKAGE_TYPE: u16 = 0x0200;
const RES_TABLE_TYPE_TYPE: u16 = 0x0201;

const UTF8_FLAG: u32 = 0x0000_0100;
const NO_ENTRY: u32 = 0xffff_ffff;
const ENTRY_FLAG_COMPLEX: u16 = 0x0001;
const VALUE_TYPE_REFERENCE: u8 = 0x01;
const VALUE_TYPE_ATTRIBUTE: u8 = 0x02;
const VALUE_TYPE_STRING: u8 = 0x03;
const VALUE_TYPE_FLOAT: u8 = 0x04;
const VALUE_TYPE_DIMENSION: u8 = 0x05;
const VALUE_TYPE_FRACTION: u8 = 0x06;
const VALUE_TYPE_INT_DEC: u8 = 0x10;
const VALUE_TYPE_INT_HEX: u8 = 0x11;
const VALUE_TYPE_INT_BOOLEAN: u8 = 0x12;
const VALUE_TYPE_FIRST_COLOR_INT: u8 = 0x1c;
const VALUE_TYPE_LAST_COLOR_INT: u8 = 0x1f;
const DEFAULT_ANDROID_SDK_VERSION: u16 = 35;
const UI_MODE_TYPE_MASK: u8 = 0x0f;
const UI_MODE_NIGHT_MASK: u8 = 0x30;
const ORIENTATION_PORT: u8 = 1;
const ORIENTATION_LAND: u8 = 2;
const SCREEN_LAYOUT_SIZE_MASK: u8 = 0x0f;
const SCREEN_LAYOUT_SIZE_SMALL: u8 = 0x01;
const SCREEN_LAYOUT_SIZE_NORMAL: u8 = 0x02;
const SCREEN_LAYOUT_SIZE_LARGE: u8 = 0x03;
const SCREEN_LAYOUT_SIZE_XLARGE: u8 = 0x04;
const SCREEN_LAYOUT_LONG_MASK: u8 = 0x30;
const SCREEN_LAYOUT_LONG_NO: u8 = 0x10;
const SCREEN_LAYOUT_LONG_YES: u8 = 0x20;
const SCREEN_LAYOUT_DIRECTION_MASK: u8 = 0xc0;
const SCREEN_LAYOUT_DIRECTION_LTR: u8 = 0x40;
const SCREEN_LAYOUT_COMPAT_NEEDED: i32 = 0x1000_0000;
const SCREEN_ROUND_MASK: u8 = 0x03;
const SCREEN_ROUND_NO: u8 = 0x01;
const SCREEN_ROUND_YES: u8 = 0x02;
const CONFIGURATION_SCREENLAYOUT_ROUND_SHIFT: i32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceTableSummary {
    pub byte_size: usize,
    pub package_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceFile {
    pub resource_id: u32,
    pub type_name: String,
    pub key_name: String,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceString {
    pub resource_id: u32,
    pub type_name: String,
    pub key_name: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceValue {
    pub resource_id: u32,
    pub type_name: String,
    pub key_name: String,
    pub value: ResourcePrimitive,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBag {
    pub parent: Option<u32>,
    pub entries: Vec<ResourceBagEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBagEntry {
    pub name: u32,
    pub value: ResourcePrimitive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceResolutionConfig {
    pub sdk_version: u16,
    pub ui_mode: u8,
    pub orientation: u8,
    pub screen_layout: u8,
    pub screen_layout2: u8,
    pub smallest_screen_width_dp: u16,
    pub screen_width_dp: u16,
    pub screen_height_dp: u16,
    pub screen_width_px: u16,
    pub screen_height_px: u16,
}

impl ResourceResolutionConfig {
    #[cfg(test)]
    pub fn android(sdk_version: u16, ui_mode: i32) -> Self {
        Self {
            sdk_version,
            ui_mode: ui_mode as u8,
            orientation: 0,
            screen_layout: 0,
            screen_layout2: 0,
            smallest_screen_width_dp: 0,
            screen_width_dp: 0,
            screen_height_dp: 0,
            screen_width_px: 0,
            screen_height_px: 0,
        }
    }

    pub fn android_with_display(sdk_version: u16, ui_mode: i32, width: u16, height: u16) -> Self {
        let orientation = if width > height {
            ORIENTATION_LAND
        } else {
            ORIENTATION_PORT
        };

        Self {
            sdk_version,
            ui_mode: ui_mode as u8,
            orientation,
            screen_layout: screen_layout_for_display(width, height) as u8,
            screen_layout2: SCREEN_ROUND_NO,
            smallest_screen_width_dp: width.min(height),
            screen_width_dp: width,
            screen_height_dp: height,
            screen_width_px: width.max(height),
            screen_height_px: width.min(height),
        }
    }

    // cite: frameworks-base DisplayContent.java#computeScreenConfiguration
    // cite: frameworks-base ResourceTypes.h#screenLayout2, MASK_SCREENROUND
    pub fn with_screen_round(mut self, round: bool) -> Self {
        self.screen_layout2 = (self.screen_layout2 & !SCREEN_ROUND_MASK)
            | if round {
                SCREEN_ROUND_YES
            } else {
                SCREEN_ROUND_NO
            };
        self
    }
}

// cite: frameworks-base Configuration.java#SCREENLAYOUT_ROUND_MASK, SCREENLAYOUT_ROUND_SHIFT
// cite: frameworks-base DisplayContent.java#computeScreenConfiguration
pub(crate) fn configuration_screen_layout_round_bits(round: bool) -> i32 {
    let round = if round {
        SCREEN_ROUND_YES
    } else {
        SCREEN_ROUND_NO
    };
    i32::from(round) << CONFIGURATION_SCREENLAYOUT_ROUND_SHIFT
}

impl Default for ResourceResolutionConfig {
    fn default() -> Self {
        Self {
            sdk_version: DEFAULT_ANDROID_SDK_VERSION,
            ui_mode: 0,
            orientation: 0,
            screen_layout: 0,
            screen_layout2: 0,
            smallest_screen_width_dp: 0,
            screen_width_dp: 0,
            screen_height_dp: 0,
            screen_width_px: 0,
            screen_height_px: 0,
        }
    }
}

pub(crate) fn screen_layout_for_display(width: u16, height: u16) -> i32 {
    // cite: frameworks-base Configuration.java#reduceScreenLayout, setLocales, setLayoutDirection
    let long_size = width.max(height);
    let short_size = width.min(height);
    let (size, long, compat_needed) = if long_size < 470 {
        (SCREEN_LAYOUT_SIZE_SMALL, SCREEN_LAYOUT_LONG_NO, false)
    } else {
        let size = if long_size >= 960 && short_size >= 720 {
            SCREEN_LAYOUT_SIZE_XLARGE
        } else if long_size >= 640 && short_size >= 480 {
            SCREEN_LAYOUT_SIZE_LARGE
        } else {
            SCREEN_LAYOUT_SIZE_NORMAL
        };
        let long = if (u32::from(long_size) * 3) / 5 >= u32::from(short_size.saturating_sub(1)) {
            SCREEN_LAYOUT_LONG_YES
        } else {
            SCREEN_LAYOUT_LONG_NO
        };
        let compat_needed = short_size > 321 || long_size > 570;
        (size, long, compat_needed)
    };

    i32::from(size | long | SCREEN_LAYOUT_DIRECTION_LTR)
        | if compat_needed {
            SCREEN_LAYOUT_COMPAT_NEEDED
        } else {
            0
        }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResourcePrimitive {
    String(String),
    Reference(u32),
    Attribute(u32),
    Float(u32),
    Dimension(u32),
    Fraction(u32),
    Integer(i32),
    Hex(u32),
    Boolean(bool),
    Color(u32),
    Array(Vec<ResourcePrimitive>),
    Bag(ResourceBag),
    Unknown { data_type: u8, data: u32 },
}

pub fn summarize_resource_table(data: &[u8]) -> Result<ResourceTableSummary> {
    let root = read_table_header(data)?;

    Ok(ResourceTableSummary {
        byte_size: data.len(),
        package_count: root.package_count,
    })
}

pub fn resource_file_index(data: &[u8]) -> Result<Vec<ResourceFile>> {
    let root = read_table_header(data)?;
    let mut offset = root.header.header_size as usize;
    let mut global_strings = None;
    let mut files = Vec::new();

    while offset < root.end {
        let header = read_chunk_header(data, offset)?;
        let chunk_end = checked_chunk_end(data, offset, header.size)?;

        match header.chunk_type {
            RES_STRING_POOL_TYPE => {
                if global_strings.is_none() {
                    global_strings = Some(cached_string_pool(data, offset, chunk_end, header)?);
                }
            }
            RES_TABLE_PACKAGE_TYPE => {
                let strings = require_global_strings(&global_strings)?;
                collect_file_resources_in_package(
                    data, offset, chunk_end, header, strings, &mut files,
                )?;
            }
            _ => {}
        }

        offset = chunk_end;
    }

    files.sort_by(|left, right| {
        left.resource_id
            .cmp(&right.resource_id)
            .then_with(|| left.path.cmp(&right.path))
    });
    Ok(files)
}

// cite: frameworks-base ResourceTypes.h#ResTable_package
pub fn resource_id_name_index(data: &[u8]) -> Result<BTreeMap<u32, String>> {
    let root = read_table_header(data)?;
    let mut offset = root.header.header_size as usize;
    let mut names = BTreeMap::new();

    while offset < root.end {
        let header = read_chunk_header(data, offset)?;
        let chunk_end = checked_chunk_end(data, offset, header.size)?;

        if header.chunk_type == RES_TABLE_PACKAGE_TYPE {
            collect_id_names_in_package(data, offset, chunk_end, header, &mut names)?;
        }

        offset = chunk_end;
    }

    Ok(names)
}

fn collect_id_names_in_package(
    data: &[u8],
    package_offset: usize,
    package_end: usize,
    header: ChunkHeader,
    names: &mut BTreeMap<u32, String>,
) -> Result<()> {
    if header.header_size < 284 {
        return Err(WinedroidError::new(format!(
            "resource package header at offset {package_offset} is too small: {}",
            header.header_size
        )));
    }

    let package_id = read_u32(data, package_offset + 8)?;
    let package_name = read_package_name(data, package_offset)?;
    let type_strings_offset = read_u32(data, package_offset + 268)? as usize;
    let key_strings_offset = read_u32(data, package_offset + 276)? as usize;
    let type_strings = read_package_string_pool(
        data,
        package_offset,
        package_end,
        type_strings_offset,
        "type string pool",
    )?;
    let key_strings = read_package_string_pool(
        data,
        package_offset,
        package_end,
        key_strings_offset,
        "key string pool",
    )?;
    let mut offset = package_offset + header.header_size as usize;

    while offset < package_end {
        let child_header = read_chunk_header(data, offset)?;
        let child_end = checked_chunk_end(data, offset, child_header.size)?;

        if child_header.chunk_type == RES_TABLE_TYPE_TYPE {
            collect_id_names_in_type(
                data,
                offset,
                child_header,
                &type_strings,
                &key_strings,
                package_id,
                &package_name,
                names,
            )?;
        }

        offset = child_end;
    }

    Ok(())
}

// cite: frameworks-base ResourceTypes.h#ResTable_type, ResTable_entry
fn collect_id_names_in_type(
    data: &[u8],
    type_offset: usize,
    header: ChunkHeader,
    type_strings: &StringPool,
    key_strings: &StringPool,
    package_id: u32,
    package_name: &str,
    names: &mut BTreeMap<u32, String>,
) -> Result<()> {
    if header.header_size < 20 {
        return Err(WinedroidError::new(format!(
            "resource type chunk at offset {type_offset} is too small: {}",
            header.header_size
        )));
    }

    let type_id = read_u8(data, type_offset + 8)?;
    if type_id == 0 {
        return Err(WinedroidError::new(format!(
            "resource type chunk at offset {type_offset} has invalid type id 0"
        )));
    }
    if type_strings.required(u32::from(type_id) - 1)? != "id" {
        return Ok(());
    }

    let entry_count = read_u32(data, type_offset + 12)?;
    let entries_start = read_u32(data, type_offset + 16)? as usize;

    for entry_index in 0..entry_count {
        let entry_offset_offset =
            type_offset + header.header_size as usize + entry_index as usize * 4;
        require_range(data, entry_offset_offset, 4, "resource entry offset")?;
        let entry_offset = read_u32(data, entry_offset_offset)?;

        if entry_offset == NO_ENTRY {
            continue;
        }

        let entry_base = type_offset
            .checked_add(entries_start)
            .and_then(|value| value.checked_add(entry_offset as usize))
            .ok_or_else(|| WinedroidError::new("resource entry offset overflow"))?;
        require_range(data, entry_base, 8, "resource entry")?;
        let key_index = read_u32(data, entry_base + 4)?;
        let key_name = key_strings.required(key_index)?;
        let resource_id = (package_id << 24) | (u32::from(type_id) << 16) | entry_index;

        names
            .entry(resource_id)
            .or_insert_with(|| format!("{package_name}:id/{key_name}"));
    }

    Ok(())
}

// cite: frameworks-base ResourceTypes.h#ResTable_package
fn read_package_name(data: &[u8], package_offset: usize) -> Result<String> {
    require_range(data, package_offset + 12, 256, "resource package name")?;
    let mut units = Vec::new();

    for index in 0..128 {
        let unit = read_u16(data, package_offset + 12 + index * 2)?;
        if unit == 0 {
            break;
        }
        units.push(unit);
    }

    String::from_utf16(&units)
        .map_err(|error| WinedroidError::with_source("resource package name is not UTF-16", error))
}

#[cfg(test)]
pub fn resolve_resource_file(data: &[u8], resource_id: u32) -> Result<Option<ResourceFile>> {
    resolve_resource_file_for_config(data, resource_id, ResourceResolutionConfig::default())
}

pub fn resolve_resource_file_for_config(
    data: &[u8],
    resource_id: u32,
    config: ResourceResolutionConfig,
) -> Result<Option<ResourceFile>> {
    Ok(
        resolve_resource_string_for_config(data, resource_id, config)?.map(|resource| {
            ResourceFile {
                resource_id: resource.resource_id,
                type_name: resource.type_name,
                key_name: resource.key_name,
                path: resource.value,
            }
        }),
    )
}

#[cfg(test)]
pub fn resolve_resource_string(data: &[u8], resource_id: u32) -> Result<Option<ResourceString>> {
    resolve_resource_string_for_config(data, resource_id, ResourceResolutionConfig::default())
}

pub fn resolve_resource_string_for_config(
    data: &[u8],
    resource_id: u32,
    config: ResourceResolutionConfig,
) -> Result<Option<ResourceString>> {
    let Some(resource) = resolve_resource_value_references_for_config(data, resource_id, config)?
    else {
        return Ok(None);
    };

    let ResourcePrimitive::String(value) = resource.value else {
        return Err(WinedroidError::new(format!(
            "resource 0x{resource_id:08x} is not string-backed"
        )));
    };

    Ok(Some(ResourceString {
        resource_id: resource.resource_id,
        type_name: resource.type_name,
        key_name: resource.key_name,
        value,
    }))
}

pub(crate) fn resolve_resource_value_references_for_config(
    data: &[u8],
    resource_id: u32,
    config: ResourceResolutionConfig,
) -> Result<Option<ResourceValue>> {
    resolve_resource_value_references_with(resource_id, |current_id| {
        resolve_resource_value_for_config(data, current_id, config)
    })
}

pub(crate) fn resolve_resource_value_references_with(
    resource_id: u32,
    mut resolve_value: impl FnMut(u32) -> Result<Option<ResourceValue>>,
) -> Result<Option<ResourceValue>> {
    const MAX_REFERENCE_COUNT: usize = 20;
    if resource_id == 0 {
        return Ok(None);
    }
    let mut current_id = resource_id;
    let mut reference_count = 0;
    let mut visited = BTreeSet::new();

    loop {
        if !visited.insert(current_id) {
            return Err(WinedroidError::new(format!(
                "resource reference cycle reached 0x{current_id:08x} from 0x{resource_id:08x}"
            )));
        }

        // cite: frameworks-base ResourceTypes.cpp#ResTable::getResource, ResTable::resolveReference
        let Some(resource) = resolve_value(current_id)? else {
            if reference_count == 0 {
                return Ok(None);
            }
            return Err(WinedroidError::new(format!(
                "resource 0x{resource_id:08x} references missing resource 0x{current_id:08x}"
            )));
        };

        let next_id = match &resource.value {
            ResourcePrimitive::Reference(next_id) => *next_id,
            _ => return Ok(Some(resource)),
        };
        if next_id == 0 {
            return Ok(Some(resource));
        }
        if reference_count == MAX_REFERENCE_COUNT {
            return Err(WinedroidError::new(format!(
                "resource 0x{resource_id:08x} exceeded {MAX_REFERENCE_COUNT} references"
            )));
        }

        reference_count += 1;
        current_id = next_id;
    }
}

pub(crate) fn resolve_resource_value_references_preserving_unresolved_with(
    resource_id: u32,
    mut resolve_value: impl FnMut(u32) -> Result<Option<ResourceValue>>,
) -> Result<Option<ResourceValue>> {
    const MAX_REFERENCE_COUNT: usize = 20;
    if resource_id == 0 {
        return Ok(None);
    }
    let mut current_id = resource_id;
    let mut reference_count = 0;

    loop {
        // cite: frameworks-base AssetManager2.cpp#AssetManager2::ResolveReference
        let Some(resource) = resolve_value(current_id)? else {
            return Ok(Some(ResourceValue {
                resource_id: current_id,
                type_name: String::new(),
                key_name: String::new(),
                value: ResourcePrimitive::Reference(current_id),
            }));
        };
        let next_id = match &resource.value {
            ResourcePrimitive::Reference(next_id) => *next_id,
            _ => return Ok(Some(resource)),
        };
        if next_id == 0 || next_id == current_id || reference_count == MAX_REFERENCE_COUNT {
            return Ok(Some(resource));
        }

        reference_count += 1;
        current_id = next_id;
    }
}

pub(crate) fn resolve_resource_value_references_for_resources_with(
    resource_id: u32,
    mut resolve_value: impl FnMut(u32) -> Result<Option<ResourceValue>>,
) -> Result<Option<ResourceValue>> {
    const MAX_REFERENCE_COUNT: usize = 20;
    if resource_id == 0 {
        return Ok(None);
    }
    let mut current_id = resource_id;
    let mut reference_count = 0;

    loop {
        // cite: frameworks-base AssetManager2.cpp#AssetManager2::ResolveReference
        // cite: frameworks-base AssetManager.java#getResourceValue
        let Some(resource) = resolve_value(current_id)? else {
            if reference_count == 0 {
                return Ok(None);
            }
            return Err(WinedroidError::new(format!(
                "resource 0x{resource_id:08x} references missing resource 0x{current_id:08x}"
            )));
        };
        let next_id = match &resource.value {
            ResourcePrimitive::Reference(next_id) => *next_id,
            _ => return Ok(Some(resource)),
        };
        if next_id == 0 || next_id == current_id || reference_count == MAX_REFERENCE_COUNT {
            return Ok(Some(resource));
        }

        reference_count += 1;
        current_id = next_id;
    }
}

pub fn resolve_resource_value(data: &[u8], resource_id: u32) -> Result<Option<ResourceValue>> {
    resolve_resource_value_for_config(data, resource_id, ResourceResolutionConfig::default())
}

pub fn find_resource_id(data: &[u8], type_name: &str, key_name: &str) -> Result<Option<u32>> {
    let root = read_table_header(data)?;
    let mut offset = root.header.header_size as usize;

    while offset < root.end {
        let header = read_chunk_header(data, offset)?;
        let chunk_end = checked_chunk_end(data, offset, header.size)?;

        if header.chunk_type == RES_TABLE_PACKAGE_TYPE {
            if let Some(resource_id) =
                find_resource_id_in_package(data, offset, chunk_end, header, type_name, key_name)?
            {
                return Ok(Some(resource_id));
            }
        }

        offset = chunk_end;
    }

    Ok(None)
}

pub fn resolve_resource_value_for_config(
    data: &[u8],
    resource_id: u32,
    config: ResourceResolutionConfig,
) -> Result<Option<ResourceValue>> {
    thread_local! {
        static VALUE_CACHE: std::cell::RefCell<
            std::collections::HashMap<(usize, usize, u32, ResourceResolutionConfig), Option<ResourceValue>>,
        > = std::cell::RefCell::new(std::collections::HashMap::new());
    }
    let key = (data.as_ptr() as usize, data.len(), resource_id, config);
    if let Some(value) = VALUE_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(value);
    }
    let value = resolve_resource_value_for_config_uncached(data, resource_id, config)?;
    VALUE_CACHE.with(|c| c.borrow_mut().insert(key, value.clone()));
    Ok(value)
}

fn resolve_resource_value_for_config_uncached(
    data: &[u8],
    resource_id: u32,
    config: ResourceResolutionConfig,
) -> Result<Option<ResourceValue>> {
    let root = read_table_header(data)?;
    let package_id = resource_id >> 24;
    let mut offset = root.header.header_size as usize;
    let mut global_strings = None;

    while offset < root.end {
        let header = read_chunk_header(data, offset)?;
        let chunk_end = checked_chunk_end(data, offset, header.size)?;

        match header.chunk_type {
            RES_STRING_POOL_TYPE => {
                if global_strings.is_none() {
                    global_strings = Some(cached_string_pool(data, offset, chunk_end, header)?);
                }
            }
            RES_TABLE_PACKAGE_TYPE => {
                let strings = require_global_strings(&global_strings)?;
                if let Some(resource) = resolve_value_in_package(
                    data,
                    offset,
                    chunk_end,
                    header,
                    strings,
                    package_id,
                    resource_id,
                    config,
                )? {
                    return Ok(Some(resource));
                }
            }
            _ => {}
        }

        offset = chunk_end;
    }

    Ok(None)
}

#[derive(Debug, Clone, Copy)]
struct TableHeader {
    header: ChunkHeader,
    end: usize,
    package_count: u32,
}

fn read_table_header(data: &[u8]) -> Result<TableHeader> {
    require_range(data, 0, 12, "resource table header")?;

    let header = read_chunk_header(data, 0)?;
    let end = checked_chunk_end(data, 0, header.size)?;
    let package_count = read_u32(data, 8)?;

    if header.chunk_type != RES_TABLE_TYPE {
        return Err(WinedroidError::new(format!(
            "resources.arsc has unexpected chunk type 0x{:04x}",
            header.chunk_type
        )));
    }

    if header.header_size < 12 {
        return Err(WinedroidError::new(format!(
            "resources.arsc header is too small: {}",
            header.header_size
        )));
    }

    Ok(TableHeader {
        header,
        end,
        package_count,
    })
}

fn require_global_strings<'a>(
    strings: &'a Option<std::rc::Rc<StringPool>>,
) -> Result<&'a StringPool> {
    strings.as_deref().ok_or_else(|| {
        WinedroidError::new("resources.arsc package appeared before global string pool")
    })
}

fn resolve_value_in_package(
    data: &[u8],
    package_offset: usize,
    package_end: usize,
    header: ChunkHeader,
    global_strings: &StringPool,
    package_id: u32,
    resource_id: u32,
    config: ResourceResolutionConfig,
) -> Result<Option<ResourceValue>> {
    if header.header_size < 284 {
        return Err(WinedroidError::new(format!(
            "resource package header at offset {package_offset} is too small: {}",
            header.header_size
        )));
    }

    let current_package_id = read_u32(data, package_offset + 8)?;
    if current_package_id != package_id {
        return Ok(None);
    }

    let type_strings_offset = read_u32(data, package_offset + 268)? as usize;
    let key_strings_offset = read_u32(data, package_offset + 276)? as usize;
    let type_strings = read_package_string_pool(
        data,
        package_offset,
        package_end,
        type_strings_offset,
        "type string pool",
    )?;
    let key_strings = read_package_string_pool(
        data,
        package_offset,
        package_end,
        key_strings_offset,
        "key string pool",
    )?;
    let target_type_id = ((resource_id >> 16) & 0xff) as u8;
    let mut offset = package_offset + header.header_size as usize;
    let mut selected = None;

    while offset < package_end {
        let child_header = read_chunk_header(data, offset)?;
        let child_end = checked_chunk_end(data, offset, child_header.size)?;

        if child_header.chunk_type == RES_TABLE_TYPE_TYPE {
            if let Some(candidate) = resolve_value_candidate_in_type(
                data,
                offset,
                child_end,
                child_header,
                global_strings,
                &type_strings,
                &key_strings,
                target_type_id,
                resource_id,
            )? {
                if candidate.config.matches(config)
                    && better_candidate(&candidate, &selected, config)
                {
                    selected = Some(candidate);
                }
            }
        }

        offset = child_end;
    }

    Ok(selected.map(|candidate| candidate.value))
}

fn collect_file_resources_in_package(
    data: &[u8],
    package_offset: usize,
    package_end: usize,
    header: ChunkHeader,
    global_strings: &StringPool,
    files: &mut Vec<ResourceFile>,
) -> Result<()> {
    if header.header_size < 284 {
        return Err(WinedroidError::new(format!(
            "resource package header at offset {package_offset} is too small: {}",
            header.header_size
        )));
    }

    let package_id = read_u32(data, package_offset + 8)?;
    let type_strings_offset = read_u32(data, package_offset + 268)? as usize;
    let key_strings_offset = read_u32(data, package_offset + 276)? as usize;
    let type_strings = read_package_string_pool(
        data,
        package_offset,
        package_end,
        type_strings_offset,
        "type string pool",
    )?;
    let key_strings = read_package_string_pool(
        data,
        package_offset,
        package_end,
        key_strings_offset,
        "key string pool",
    )?;
    let mut offset = package_offset + header.header_size as usize;

    while offset < package_end {
        let child_header = read_chunk_header(data, offset)?;
        let child_end = checked_chunk_end(data, offset, child_header.size)?;

        if child_header.chunk_type == RES_TABLE_TYPE_TYPE {
            collect_file_resources_in_type(
                data,
                offset,
                child_end,
                child_header,
                global_strings,
                &type_strings,
                &key_strings,
                package_id,
                files,
            )?;
        }

        offset = child_end;
    }

    Ok(())
}

fn find_resource_id_in_package(
    data: &[u8],
    package_offset: usize,
    package_end: usize,
    header: ChunkHeader,
    target_type_name: &str,
    target_key_name: &str,
) -> Result<Option<u32>> {
    if header.header_size < 284 {
        return Err(WinedroidError::new(format!(
            "resource package header at offset {package_offset} is too small: {}",
            header.header_size
        )));
    }

    let package_id = read_u32(data, package_offset + 8)?;
    let type_strings_offset = read_u32(data, package_offset + 268)? as usize;
    let key_strings_offset = read_u32(data, package_offset + 276)? as usize;
    let type_strings = read_package_string_pool(
        data,
        package_offset,
        package_end,
        type_strings_offset,
        "type string pool",
    )?;
    let key_strings = read_package_string_pool(
        data,
        package_offset,
        package_end,
        key_strings_offset,
        "key string pool",
    )?;
    let mut offset = package_offset + header.header_size as usize;

    while offset < package_end {
        let child_header = read_chunk_header(data, offset)?;
        let child_end = checked_chunk_end(data, offset, child_header.size)?;

        if child_header.chunk_type == RES_TABLE_TYPE_TYPE {
            if let Some(resource_id) = find_resource_id_in_type(
                data,
                offset,
                child_end,
                child_header,
                &type_strings,
                &key_strings,
                package_id,
                target_type_name,
                target_key_name,
            )? {
                return Ok(Some(resource_id));
            }
        }

        offset = child_end;
    }

    Ok(None)
}

fn find_resource_id_in_type(
    data: &[u8],
    type_offset: usize,
    type_end: usize,
    header: ChunkHeader,
    type_strings: &StringPool,
    key_strings: &StringPool,
    package_id: u32,
    target_type_name: &str,
    target_key_name: &str,
) -> Result<Option<u32>> {
    if header.header_size < 20 {
        return Err(WinedroidError::new(format!(
            "resource type chunk at offset {type_offset} is too small: {}",
            header.header_size
        )));
    }

    let type_id = read_u8(data, type_offset + 8)?;
    if type_id == 0 {
        return Err(WinedroidError::new(format!(
            "resource type chunk at offset {type_offset} has invalid type id 0"
        )));
    }

    let type_name = type_strings.required(u32::from(type_id - 1))?;
    if type_name != target_type_name {
        return Ok(None);
    }

    let entry_count = read_u32(data, type_offset + 12)?;
    let entries_start = read_u32(data, type_offset + 16)? as usize;

    for entry_index in 0..entry_count {
        let entry_offset_offset =
            type_offset + header.header_size as usize + entry_index as usize * 4;
        require_range(data, entry_offset_offset, 4, "resource entry offset")?;
        let entry_offset = read_u32(data, entry_offset_offset)?;

        if entry_offset == NO_ENTRY {
            continue;
        }

        let entry_base = type_offset
            .checked_add(entries_start)
            .and_then(|value| value.checked_add(entry_offset as usize))
            .ok_or_else(|| WinedroidError::new("resource entry offset overflow"))?;
        require_range(data, entry_base, 8, "resource entry")?;
        if entry_base >= type_end {
            return Err(WinedroidError::new(format!(
                "resource entry index {entry_index} starts outside its type chunk"
            )));
        }

        let key_index = read_u32(data, entry_base + 4)?;
        let key_name = key_strings.required(key_index)?;
        if key_name == target_key_name {
            return Ok(Some(
                (package_id << 24) | (u32::from(type_id) << 16) | entry_index,
            ));
        }
    }

    Ok(None)
}

fn read_package_string_pool(
    data: &[u8],
    package_offset: usize,
    package_end: usize,
    relative_offset: usize,
    label: &str,
) -> Result<std::rc::Rc<StringPool>> {
    if relative_offset == 0 {
        return Err(WinedroidError::new(format!(
            "resource package has no {label}"
        )));
    }

    let offset = package_offset
        .checked_add(relative_offset)
        .ok_or_else(|| WinedroidError::new(format!("{label} offset overflow")))?;

    if offset >= package_end {
        return Err(WinedroidError::new(format!(
            "{label} starts outside its resource package"
        )));
    }

    let header = read_chunk_header(data, offset)?;
    let chunk_end = checked_chunk_end(data, offset, header.size)?;

    if chunk_end > package_end {
        return Err(WinedroidError::new(format!(
            "{label} extends outside its resource package"
        )));
    }

    if header.chunk_type != RES_STRING_POOL_TYPE {
        return Err(WinedroidError::new(format!(
            "{label} has unexpected chunk type 0x{:04x}",
            header.chunk_type
        )));
    }

    cached_string_pool(data, offset, chunk_end, header)
}

fn collect_file_resources_in_type(
    data: &[u8],
    type_offset: usize,
    type_end: usize,
    header: ChunkHeader,
    global_strings: &StringPool,
    type_strings: &StringPool,
    key_strings: &StringPool,
    package_id: u32,
    files: &mut Vec<ResourceFile>,
) -> Result<()> {
    if header.header_size < 20 {
        return Err(WinedroidError::new(format!(
            "resource type chunk at offset {type_offset} is too small: {}",
            header.header_size
        )));
    }

    let type_id = read_u8(data, type_offset + 8)?;
    if type_id == 0 {
        return Err(WinedroidError::new(format!(
            "resource type chunk at offset {type_offset} has invalid type id 0"
        )));
    }

    let entry_count = read_u32(data, type_offset + 12)?;
    let entries_start = read_u32(data, type_offset + 16)? as usize;

    for entry_index in 0..entry_count {
        let entry_offset_offset =
            type_offset + header.header_size as usize + entry_index as usize * 4;
        require_range(data, entry_offset_offset, 4, "resource entry offset")?;
        let entry_offset = read_u32(data, entry_offset_offset)?;

        if entry_offset == NO_ENTRY {
            continue;
        }

        let entry_base = type_offset
            .checked_add(entries_start)
            .and_then(|value| value.checked_add(entry_offset as usize))
            .ok_or_else(|| WinedroidError::new("resource entry offset overflow"))?;
        let resource_id = (package_id << 24) | (u32::from(type_id) << 16) | entry_index;
        let resource = read_value_entry(
            data,
            entry_base,
            type_end,
            global_strings,
            type_strings,
            key_strings,
            type_id,
            resource_id,
        )?;
        let ResourcePrimitive::String(path) = resource.value else {
            continue;
        };
        if !path.starts_with("res/") {
            continue;
        }

        files.push(ResourceFile {
            resource_id: resource.resource_id,
            type_name: resource.type_name,
            key_name: resource.key_name,
            path,
        });
    }

    Ok(())
}

fn resolve_value_candidate_in_type(
    data: &[u8],
    type_offset: usize,
    type_end: usize,
    header: ChunkHeader,
    global_strings: &StringPool,
    type_strings: &StringPool,
    key_strings: &StringPool,
    target_type_id: u8,
    resource_id: u32,
) -> Result<Option<ResourceValueCandidate>> {
    if header.header_size < 20 {
        return Err(WinedroidError::new(format!(
            "resource type chunk at offset {type_offset} is too small: {}",
            header.header_size
        )));
    }

    let type_id = read_u8(data, type_offset + 8)?;
    if type_id != target_type_id {
        return Ok(None);
    }

    let entry_count = read_u32(data, type_offset + 12)?;
    let entries_start = read_u32(data, type_offset + 16)? as usize;
    let target_entry_index = resource_id & 0xffff;
    let config = read_type_config(data, type_offset, header)?;

    if target_entry_index >= entry_count {
        return Ok(None);
    }

    let entry_offset_offset =
        type_offset + header.header_size as usize + target_entry_index as usize * 4;
    require_range(data, entry_offset_offset, 4, "resource entry offset")?;
    let entry_offset = read_u32(data, entry_offset_offset)?;

    if entry_offset == NO_ENTRY {
        return Ok(None);
    }

    let entry_base = type_offset
        .checked_add(entries_start)
        .and_then(|value| value.checked_add(entry_offset as usize))
        .ok_or_else(|| WinedroidError::new("resource entry offset overflow"))?;

    if entry_base >= type_end {
        return Err(WinedroidError::new(format!(
            "resource entry 0x{resource_id:08x} starts outside its type chunk"
        )));
    }

    let value = read_value_entry(
        data,
        entry_base,
        type_end,
        global_strings,
        type_strings,
        key_strings,
        type_id,
        resource_id,
    )
    .map(Some)?;

    Ok(value.map(|value| ResourceValueCandidate { config, value }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResourceValueCandidate {
    config: ResourceTypeConfig,
    value: ResourceValue,
}

fn better_candidate(
    candidate: &ResourceValueCandidate,
    selected: &Option<ResourceValueCandidate>,
    requested: ResourceResolutionConfig,
) -> bool {
    let Some(selected) = selected else {
        return true;
    };

    candidate.config.score(requested) > selected.config.score(requested)
}

fn read_value_entry(
    data: &[u8],
    entry_base: usize,
    type_end: usize,
    global_strings: &StringPool,
    type_strings: &StringPool,
    key_strings: &StringPool,
    type_id: u8,
    resource_id: u32,
) -> Result<ResourceValue> {
    require_range(data, entry_base, 8, "resource entry")?;

    let entry_size = read_u16(data, entry_base)? as usize;
    let flags = read_u16(data, entry_base + 2)?;
    let key_index = read_u32(data, entry_base + 4)?;

    if type_id == 0 {
        return Err(WinedroidError::new(format!(
            "resource 0x{resource_id:08x} has invalid type id 0"
        )));
    }

    let type_name = type_strings.required(u32::from(type_id - 1))?;
    let key_name = key_strings.required(key_index)?;

    if flags & ENTRY_FLAG_COMPLEX != 0 {
        let bag = read_complex_resource_bag(data, entry_base, type_end, global_strings)?;
        let value = if is_array_type_name(&type_name) {
            ResourcePrimitive::Array(bag.entries.into_iter().map(|entry| entry.value).collect())
        } else {
            ResourcePrimitive::Bag(bag)
        };

        return Ok(ResourceValue {
            resource_id,
            type_name,
            key_name,
            value,
        });
    }

    let value_offset = entry_base
        .checked_add(entry_size)
        .ok_or_else(|| WinedroidError::new("resource value offset overflow"))?;

    if value_offset + 8 > type_end {
        return Err(WinedroidError::new(format!(
            "resource value for 0x{resource_id:08x} extends outside its type chunk"
        )));
    }

    let value_size = read_u16(data, value_offset)?;
    let data_type = read_u8(data, value_offset + 3)?;
    let data = read_u32(data, value_offset + 4)?;

    if value_size < 8 {
        return Err(WinedroidError::new(format!(
            "resource value for 0x{resource_id:08x} is too small: {value_size}"
        )));
    }

    Ok(ResourceValue {
        resource_id,
        type_name,
        key_name,
        value: parse_resource_primitive(data_type, data, global_strings)?,
    })
}

fn read_complex_resource_bag(
    data: &[u8],
    entry_base: usize,
    type_end: usize,
    global_strings: &StringPool,
) -> Result<ResourceBag> {
    require_range(data, entry_base, 16, "complex resource entry")?;

    let entry_size = read_u16(data, entry_base)? as usize;
    if entry_size < 16 {
        return Err(WinedroidError::new(format!(
            "complex resource entry is too small: {entry_size}"
        )));
    }

    let parent = read_u32(data, entry_base + 8)?;
    let parent = if parent == 0 { None } else { Some(parent) };
    let map_count_offset = entry_base
        .checked_add(12)
        .ok_or_else(|| WinedroidError::new("complex resource map count offset overflow"))?;
    require_range(data, map_count_offset, 4, "complex resource map count")?;

    let map_count = read_u32(data, map_count_offset)? as usize;
    let maps_offset = entry_base
        .checked_add(entry_size)
        .ok_or_else(|| WinedroidError::new("complex resource map offset overflow"))?;
    let maps_size = map_count
        .checked_mul(12)
        .ok_or_else(|| WinedroidError::new("complex resource maps size overflow"))?;
    let maps_end = maps_offset
        .checked_add(maps_size)
        .ok_or_else(|| WinedroidError::new("complex resource maps end overflow"))?;
    require_range(data, maps_offset, maps_size, "complex resource maps")?;

    if maps_end > type_end {
        return Err(WinedroidError::new(
            "complex resource maps extend outside their type chunk",
        ));
    }

    let mut entries = Vec::with_capacity(map_count);
    for index in 0..map_count {
        let map_offset = maps_offset
            .checked_add(index.saturating_mul(12))
            .ok_or_else(|| WinedroidError::new("complex resource map offset overflow"))?;
        let name = read_u32(data, map_offset)?;
        let value_offset = map_offset
            .checked_add(4)
            .ok_or_else(|| WinedroidError::new("complex resource map value offset overflow"))?;
        let value_size = read_u16(data, value_offset)?;
        let data_type = read_u8(data, value_offset + 3)?;
        let data = read_u32(data, value_offset + 4)?;

        if value_size < 8 {
            return Err(WinedroidError::new(format!(
                "complex resource map value {index} is too small: {value_size}"
            )));
        }

        entries.push(ResourceBagEntry {
            name,
            value: parse_resource_primitive(data_type, data, global_strings)?,
        });
    }

    Ok(ResourceBag { parent, entries })
}

fn is_array_type_name(type_name: &str) -> bool {
    type_name == "array" || type_name.ends_with("-array")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResourceTypeConfig {
    sdk_version: u16,
    ui_mode: u8,
    orientation: u8,
    screen_layout: u8,
    screen_layout2: u8,
    smallest_screen_width_dp: u16,
    screen_width_dp: u16,
    screen_height_dp: u16,
    screen_width_px: u16,
    screen_height_px: u16,
}

impl ResourceTypeConfig {
    fn default() -> Self {
        Self {
            sdk_version: 0,
            ui_mode: 0,
            orientation: 0,
            screen_layout: 0,
            screen_layout2: 0,
            smallest_screen_width_dp: 0,
            screen_width_dp: 0,
            screen_height_dp: 0,
            screen_width_px: 0,
            screen_height_px: 0,
        }
    }

    fn matches(self, requested: ResourceResolutionConfig) -> bool {
        // cite: frameworks-base ResourceTypes.cpp#ResTable_config::match
        if self.sdk_version > requested.sdk_version {
            return false;
        }

        let requested_ui_mode_type = requested.ui_mode & UI_MODE_TYPE_MASK;
        let candidate_ui_mode_type = self.ui_mode & UI_MODE_TYPE_MASK;
        if candidate_ui_mode_type != 0 && candidate_ui_mode_type != requested_ui_mode_type {
            return false;
        }

        let requested_night = requested.ui_mode & UI_MODE_NIGHT_MASK;
        let candidate_night = self.ui_mode & UI_MODE_NIGHT_MASK;
        if candidate_night != 0 && candidate_night != requested_night {
            return false;
        }

        let requested_round = requested.screen_layout2 & SCREEN_ROUND_MASK;
        let candidate_round = self.screen_layout2 & SCREEN_ROUND_MASK;
        if candidate_round != 0 && candidate_round != requested_round {
            return false;
        }

        if self.orientation != 0 && self.orientation != requested.orientation {
            return false;
        }

        let candidate_layout_direction = self.screen_layout & SCREEN_LAYOUT_DIRECTION_MASK;
        let requested_layout_direction = requested.screen_layout & SCREEN_LAYOUT_DIRECTION_MASK;
        if candidate_layout_direction != 0
            && candidate_layout_direction != requested_layout_direction
        {
            return false;
        }

        let candidate_screen_size = self.screen_layout & SCREEN_LAYOUT_SIZE_MASK;
        let requested_screen_size = requested.screen_layout & SCREEN_LAYOUT_SIZE_MASK;
        if candidate_screen_size != 0 && candidate_screen_size > requested_screen_size {
            return false;
        }

        let candidate_screen_long = self.screen_layout & SCREEN_LAYOUT_LONG_MASK;
        let requested_screen_long = requested.screen_layout & SCREEN_LAYOUT_LONG_MASK;
        if candidate_screen_long != 0 && candidate_screen_long != requested_screen_long {
            return false;
        }

        if self.smallest_screen_width_dp > requested.smallest_screen_width_dp
            || self.screen_width_dp > requested.screen_width_dp
            || self.screen_height_dp > requested.screen_height_dp
            || self.screen_width_px > requested.screen_width_px
            || self.screen_height_px > requested.screen_height_px
        {
            return false;
        }

        true
    }

    fn score(self, requested: ResourceResolutionConfig) -> ResourceTypeConfigScore {
        // cite: frameworks-base ResourceTypes.cpp#ResTable_config::isBetterThan
        let requested_night = requested.ui_mode & UI_MODE_NIGHT_MASK;
        let candidate_night = self.ui_mode & UI_MODE_NIGHT_MASK;
        let night_score = u8::from(candidate_night != 0 && candidate_night == requested_night);
        let ui_mode_type_score = u8::from(
            self.ui_mode & UI_MODE_TYPE_MASK != 0 && requested.ui_mode & UI_MODE_TYPE_MASK != 0,
        );
        let screen_round_score = u8::from(
            self.screen_layout2 & SCREEN_ROUND_MASK != 0
                && requested.screen_layout2 & SCREEN_ROUND_MASK != 0,
        );
        let orientation_score = u8::from(self.orientation != 0);
        let layout_direction_score =
            u8::from(self.screen_layout & SCREEN_LAYOUT_DIRECTION_MASK != 0);
        let candidate_screen_size = self.screen_layout & SCREEN_LAYOUT_SIZE_MASK;
        let requested_screen_size = requested.screen_layout & SCREEN_LAYOUT_SIZE_MASK;
        let screen_size_score =
            if requested_screen_size >= SCREEN_LAYOUT_SIZE_NORMAL && candidate_screen_size == 0 {
                SCREEN_LAYOUT_SIZE_NORMAL
            } else {
                candidate_screen_size
            };
        let screen_size_explicit = u8::from(candidate_screen_size != 0);
        let screen_long_score = u8::from(self.screen_layout & SCREEN_LAYOUT_LONG_MASK != 0);
        let screen_size_delta = u32::from(
            requested
                .screen_width_dp
                .saturating_sub(self.screen_width_dp),
        ) + u32::from(
            requested
                .screen_height_dp
                .saturating_sub(self.screen_height_dp),
        );
        let legacy_screen_size_delta = u32::from(
            requested
                .screen_width_px
                .saturating_sub(self.screen_width_px),
        ) + u32::from(
            requested
                .screen_height_px
                .saturating_sub(self.screen_height_px),
        );

        ResourceTypeConfigScore {
            layout_direction_score,
            smallest_screen_width_dp: self.smallest_screen_width_dp,
            screen_size_closeness: u32::MAX - screen_size_delta,
            screen_size_score,
            screen_size_explicit,
            screen_long_score,
            screen_round_score,
            orientation_score,
            ui_mode_type_score,
            night_score,
            legacy_screen_size_closeness: u32::MAX - legacy_screen_size_delta,
            sdk_version: self.sdk_version,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ResourceTypeConfigScore {
    layout_direction_score: u8,
    smallest_screen_width_dp: u16,
    screen_size_closeness: u32,
    screen_size_score: u8,
    screen_size_explicit: u8,
    screen_long_score: u8,
    screen_round_score: u8,
    orientation_score: u8,
    ui_mode_type_score: u8,
    night_score: u8,
    legacy_screen_size_closeness: u32,
    sdk_version: u16,
}

fn read_type_config(
    data: &[u8],
    type_offset: usize,
    header: ChunkHeader,
) -> Result<ResourceTypeConfig> {
    if header.header_size <= 20 {
        return Ok(ResourceTypeConfig::default());
    }

    let config_offset = type_offset
        .checked_add(20)
        .ok_or_else(|| WinedroidError::new("resource type config offset overflow"))?;
    let available = usize::from(header.header_size) - 20;
    require_range(data, config_offset, available, "resource type config")?;

    if available < 4 {
        return Err(WinedroidError::new(format!(
            "resource type config at offset {config_offset} is too small: {available}"
        )));
    }

    let config_size = read_u32(data, config_offset)? as usize;
    if config_size > available {
        return Err(WinedroidError::new(format!(
            "resource type config at offset {config_offset} declares size {config_size}, but only {available} bytes are available"
        )));
    }

    let sdk_version = if config_size >= 28 {
        read_u16(data, config_offset + 24)?
    } else {
        0
    };
    let ui_mode = if config_size >= 30 {
        read_u8(data, config_offset + 29)?
    } else {
        0
    };
    let screen_layout = if config_size >= 30 {
        read_u8(data, config_offset + 28)?
    } else {
        0
    };
    let orientation = if config_size >= 16 {
        read_u8(data, config_offset + 12)?
    } else {
        0
    };
    let smallest_screen_width_dp = if config_size >= 32 {
        read_u16(data, config_offset + 30)?
    } else {
        0
    };
    let (screen_width_dp, screen_height_dp) = if config_size >= 36 {
        (
            read_u16(data, config_offset + 32)?,
            read_u16(data, config_offset + 34)?,
        )
    } else {
        (0, 0)
    };
    let screen_layout2 = if config_size >= 49 {
        read_u8(data, config_offset + 48)?
    } else {
        0
    };
    let (screen_width_px, screen_height_px) = if config_size >= 24 {
        (
            read_u16(data, config_offset + 20)?,
            read_u16(data, config_offset + 22)?,
        )
    } else {
        (0, 0)
    };

    // cite: frameworks-base ResourceTypes.h#ResTable_config
    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::match, ResTable_config::isBetterThan
    Ok(ResourceTypeConfig {
        sdk_version,
        ui_mode,
        orientation,
        screen_layout,
        screen_layout2,
        smallest_screen_width_dp,
        screen_width_dp,
        screen_height_dp,
        screen_width_px,
        screen_height_px,
    })
}

fn parse_resource_primitive(
    data_type: u8,
    data: u32,
    global_strings: &StringPool,
) -> Result<ResourcePrimitive> {
    match data_type {
        VALUE_TYPE_REFERENCE => Ok(ResourcePrimitive::Reference(data)),
        VALUE_TYPE_ATTRIBUTE => Ok(ResourcePrimitive::Attribute(data)),
        VALUE_TYPE_STRING => Ok(ResourcePrimitive::String(global_strings.required(data)?)),
        VALUE_TYPE_FLOAT => Ok(ResourcePrimitive::Float(data)),
        VALUE_TYPE_DIMENSION => Ok(ResourcePrimitive::Dimension(data)),
        VALUE_TYPE_FRACTION => Ok(ResourcePrimitive::Fraction(data)),
        VALUE_TYPE_INT_DEC => Ok(ResourcePrimitive::Integer(data as i32)),
        VALUE_TYPE_INT_HEX => Ok(ResourcePrimitive::Hex(data)),
        VALUE_TYPE_INT_BOOLEAN => Ok(ResourcePrimitive::Boolean(data != 0)),
        VALUE_TYPE_FIRST_COLOR_INT..=VALUE_TYPE_LAST_COLOR_INT => {
            Ok(ResourcePrimitive::Color(data))
        }
        _ => Ok(ResourcePrimitive::Unknown { data_type, data }),
    }
}

#[derive(Debug, Clone, Copy)]
struct ChunkHeader {
    chunk_type: u16,
    header_size: u16,
    size: u32,
}

fn read_chunk_header(data: &[u8], offset: usize) -> Result<ChunkHeader> {
    require_range(data, offset, 8, "chunk header")?;

    let header = ChunkHeader {
        chunk_type: read_u16(data, offset)?,
        header_size: read_u16(data, offset + 2)?,
        size: read_u32(data, offset + 4)?,
    };

    if header.header_size < 8 {
        return Err(WinedroidError::new(format!(
            "chunk at offset {offset} has invalid header size {}",
            header.header_size
        )));
    }

    if header.size < u32::from(header.header_size) {
        return Err(WinedroidError::new(format!(
            "chunk at offset {offset} has invalid size {}",
            header.size
        )));
    }

    Ok(header)
}

fn checked_chunk_end(data: &[u8], offset: usize, size: u32) -> Result<usize> {
    let size = size as usize;
    let chunk_end = offset
        .checked_add(size)
        .ok_or_else(|| WinedroidError::new("chunk size overflow"))?;

    if chunk_end > data.len() {
        return Err(WinedroidError::new(format!(
            "chunk at offset {offset} extends past the end of the file"
        )));
    }

    Ok(chunk_end)
}

#[derive(Debug, Clone)]
struct StringPool {
    strings: Vec<String>,
}

fn cached_string_pool(
    data: &[u8],
    offset: usize,
    chunk_end: usize,
    header: ChunkHeader,
) -> Result<std::rc::Rc<StringPool>> {
    use std::cell::RefCell;
    use std::rc::Rc;
    thread_local! {
        static POOLS: RefCell<Vec<((usize, usize, usize), Rc<StringPool>)>> =
            const { RefCell::new(Vec::new()) };
    }
    let key = (data.as_ptr() as usize, data.len(), offset);
    POOLS.with(|pools| {
        if let Some((_, pool)) = pools.borrow().iter().find(|(k, _)| *k == key) {
            return Ok(Rc::clone(pool));
        }
        let pool = Rc::new(StringPool::parse(data, offset, chunk_end, header)?);
        pools.borrow_mut().push((key, Rc::clone(&pool)));
        Ok(pool)
    })
}

impl StringPool {
    fn parse(data: &[u8], offset: usize, chunk_end: usize, header: ChunkHeader) -> Result<Self> {
        if header.header_size < 28 {
            return Err(WinedroidError::new(format!(
                "string pool header is too small at offset {offset}"
            )));
        }

        let string_count = read_u32(data, offset + 8)? as usize;
        let flags = read_u32(data, offset + 16)?;
        let strings_start = read_u32(data, offset + 20)? as usize;
        let offsets_start = offset + header.header_size as usize;
        let string_data_start = offset + strings_start;
        let is_utf8 = flags & UTF8_FLAG != 0;

        require_range(data, offsets_start, string_count * 4, "string offsets")?;

        if string_data_start > chunk_end {
            return Err(WinedroidError::new(format!(
                "string data for pool at offset {offset} starts outside its chunk"
            )));
        }

        let mut strings = Vec::with_capacity(string_count);

        for index in 0..string_count {
            let relative_offset = read_u32(data, offsets_start + index * 4)? as usize;
            let string_offset = string_data_start + relative_offset;

            if string_offset >= chunk_end {
                return Err(WinedroidError::new(format!(
                    "string {index} starts outside the string pool"
                )));
            }

            let bytes = &data[string_offset..chunk_end];
            let string = if is_utf8 {
                decode_utf8_string(bytes)?
            } else {
                decode_utf16_string(bytes)?
            };

            strings.push(string);
        }

        Ok(Self { strings })
    }

    fn required(&self, index: u32) -> Result<String> {
        self.strings
            .get(index as usize)
            .cloned()
            .ok_or_else(|| WinedroidError::new(format!("string pool index {index} is invalid")))
    }
}

fn decode_utf8_string(data: &[u8]) -> Result<String> {
    let (_, first_length_size) = read_utf8_length(data, 0)?;
    let (byte_length, second_length_size) = read_utf8_length(data, first_length_size)?;
    let value_offset = first_length_size + second_length_size;
    let terminator_offset = value_offset + byte_length;

    require_range(data, value_offset, byte_length + 1, "UTF-8 string")?;

    if data[terminator_offset] != 0 {
        return Err(WinedroidError::new("UTF-8 string is not null-terminated"));
    }

    let value = std::str::from_utf8(&data[value_offset..terminator_offset])
        .map_err(|error| WinedroidError::with_source("invalid UTF-8 string", error))?;

    Ok(value.to_owned())
}

fn decode_utf16_string(data: &[u8]) -> Result<String> {
    let (code_unit_count, length_size) = read_utf16_length(data, 0)?;
    let value_offset = length_size;
    let byte_length = code_unit_count * 2;
    let terminator_offset = value_offset + byte_length;

    require_range(data, value_offset, byte_length + 2, "UTF-16 string")?;

    if data[terminator_offset] != 0 || data[terminator_offset + 1] != 0 {
        return Err(WinedroidError::new("UTF-16 string is not null-terminated"));
    }

    let mut code_units = Vec::with_capacity(code_unit_count);

    for index in 0..code_unit_count {
        code_units.push(read_u16(data, value_offset + index * 2)?);
    }

    String::from_utf16(&code_units)
        .map_err(|error| WinedroidError::with_source("invalid UTF-16 string", error))
}

fn read_utf8_length(data: &[u8], offset: usize) -> Result<(usize, usize)> {
    let first = read_u8(data, offset)?;

    if first & 0x80 == 0 {
        return Ok((first as usize, 1));
    }

    let second = read_u8(data, offset + 1)?;
    let value = (((first & 0x7f) as usize) << 8) | second as usize;

    Ok((value, 2))
}

fn read_utf16_length(data: &[u8], offset: usize) -> Result<(usize, usize)> {
    let first = read_u16(data, offset)?;

    if first & 0x8000 == 0 {
        return Ok((first as usize, 2));
    }

    let second = read_u16(data, offset + 2)?;
    let value = (((first & 0x7fff) as usize) << 16) | second as usize;

    Ok((value, 4))
}

fn read_u8(data: &[u8], offset: usize) -> Result<u8> {
    require_range(data, offset, 1, "u8")?;
    Ok(data[offset])
}

fn read_u16(data: &[u8], offset: usize) -> Result<u16> {
    require_range(data, offset, 2, "u16")?;
    Ok(u16::from_le_bytes([data[offset], data[offset + 1]]))
}

fn read_u32(data: &[u8], offset: usize) -> Result<u32> {
    require_range(data, offset, 4, "u32")?;
    Ok(u32::from_le_bytes([
        data[offset],
        data[offset + 1],
        data[offset + 2],
        data[offset + 3],
    ]))
}

fn require_range(data: &[u8], offset: usize, length: usize, label: &str) -> Result<()> {
    let end = offset
        .checked_add(length)
        .ok_or_else(|| WinedroidError::new(format!("{label} range overflow")))?;

    if end > data.len() {
        return Err(WinedroidError::new(format!(
            "{label} at offset {offset} extends past the end of the file"
        )));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_resource_table_package_count() {
        let data = [
            0x02, 0x00, 0x0c, 0x00, 0x0c, 0x00, 0x00, 0x00, 0x02, 0x00, 0x00, 0x00,
        ];

        let summary = summarize_resource_table(&data).expect("resource table summary should parse");

        assert_eq!(summary.byte_size, 12);
        assert_eq!(summary.package_count, 2);
    }

    #[test]
    fn resolves_simple_file_backed_resource() {
        let data = minimal_resource_table("layout", "main", "res/layout/main.xml");
        let file = resolve_resource_file(&data, 0x7f010000)
            .expect("resource table should parse")
            .expect("resource should resolve");

        assert_eq!(
            file,
            ResourceFile {
                resource_id: 0x7f010000,
                type_name: "layout".to_owned(),
                key_name: "main".to_owned(),
                path: "res/layout/main.xml".to_owned(),
            }
        );
    }

    #[test]
    fn resolves_simple_string_resource() {
        let data = minimal_resource_table("string", "welcome", "Welcome");
        let string = resolve_resource_string(&data, 0x7f010000)
            .expect("resource table should parse")
            .expect("resource should resolve");

        assert_eq!(
            string,
            ResourceString {
                resource_id: 0x7f010000,
                type_name: "string".to_owned(),
                key_name: "welcome".to_owned(),
                value: "Welcome".to_owned(),
            }
        );
    }

    #[test]
    fn finds_resource_id_by_type_and_key_name() {
        let data = minimal_resource_table("attr", "button_color", "unused");

        assert_eq!(
            find_resource_id(&data, "attr", "button_color").expect("resource table should parse"),
            Some(0x7f010000)
        );
        assert_eq!(
            find_resource_id(&data, "color", "button_color").expect("resource table should parse"),
            None
        );
    }

    #[test]
    fn resolves_simple_float_resource() {
        let data = minimal_typed_resource_table(
            "dimen",
            "button_scale",
            VALUE_TYPE_FLOAT,
            0.8_f32.to_bits(),
        );
        let resource = resolve_resource_value(&data, 0x7f010000)
            .expect("resource table should parse")
            .expect("resource should resolve");

        assert_eq!(
            resource,
            ResourceValue {
                resource_id: 0x7f010000,
                type_name: "dimen".to_owned(),
                key_name: "button_scale".to_owned(),
                value: ResourcePrimitive::Float(0.8_f32.to_bits()),
            }
        );
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
    #[test]
    fn resource_reference_resolution_treats_zero_as_terminal_null() {
        let data =
            minimal_typed_resource_table("drawable", "optional_icon", VALUE_TYPE_REFERENCE, 0);
        let resource = resolve_resource_value_references_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::default(),
        )
        .expect("null resource reference should parse")
        .expect("null resource reference should remain a typed value");

        assert_eq!(resource.value, ResourcePrimitive::Reference(0));
    }

    #[test]
    fn resolves_complex_string_array_resource() {
        let data = minimal_array_resource_table(
            "array",
            "decimal_places",
            &["10", "50"],
            &[(VALUE_TYPE_STRING, 0), (VALUE_TYPE_STRING, 1)],
        );
        let resource = resolve_resource_value(&data, 0x7f010000)
            .expect("resource table should parse")
            .expect("resource should resolve");

        assert_eq!(
            resource,
            ResourceValue {
                resource_id: 0x7f010000,
                type_name: "array".to_owned(),
                key_name: "decimal_places".to_owned(),
                value: ResourcePrimitive::Array(vec![
                    ResourcePrimitive::String("10".to_owned()),
                    ResourcePrimitive::String("50".to_owned()),
                ]),
            }
        );
    }

    #[test]
    fn resolves_complex_style_bag_resource() {
        let data = minimal_resource_table_with_type_chunk(
            string_pool(&[]),
            "style",
            "AppTheme",
            style_type_chunk(
                Some(0x7f01_0123),
                &[
                    (0x7f02_0001, VALUE_TYPE_REFERENCE, 0x7f03_0000),
                    (0x0101_0031, VALUE_TYPE_INT_BOOLEAN, 0xffff_ffff),
                ],
            ),
        );
        let resource = resolve_resource_value(&data, 0x7f01_0000)
            .expect("resource table should parse")
            .expect("resource should resolve");

        assert_eq!(
            resource,
            ResourceValue {
                resource_id: 0x7f01_0000,
                type_name: "style".to_owned(),
                key_name: "AppTheme".to_owned(),
                value: ResourcePrimitive::Bag(ResourceBag {
                    parent: Some(0x7f01_0123),
                    entries: vec![
                        ResourceBagEntry {
                            name: 0x7f02_0001,
                            value: ResourcePrimitive::Reference(0x7f03_0000),
                        },
                        ResourceBagEntry {
                            name: 0x0101_0031,
                            value: ResourcePrimitive::Boolean(true),
                        },
                    ],
                }),
            }
        );
    }

    #[test]
    fn resolves_night_value_when_requested_by_ui_mode() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk(VALUE_TYPE_FIRST_COLOR_INT, 0xffff_ffff),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff12_1212,
                    res_table_config(0, 0x20),
                ),
            ],
        );

        let default_resource = resolve_resource_value(&data, 0x7f01_0000)
            .expect("resource table should parse")
            .expect("resource should resolve");
        let dark_resource = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android(35, 0x21),
        )
        .expect("resource table should parse")
        .expect("resource should resolve");

        assert_eq!(
            default_resource.value,
            ResourcePrimitive::Color(0xffff_ffff)
        );
        assert_eq!(dark_resource.value, ResourcePrimitive::Color(0xff12_1212));
    }

    #[test]
    fn resolves_versioned_night_value_only_when_sdk_matches() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk(VALUE_TYPE_FIRST_COLOR_INT, 0xffff_ffff),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff11_1111,
                    res_table_config(0, 0x20),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff22_2222,
                    res_table_config(23, 0x20),
                ),
            ],
        );

        let sdk22_resource = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android(22, 0x21),
        )
        .expect("resource table should parse")
        .expect("resource should resolve");
        let sdk35_resource = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android(35, 0x21),
        )
        .expect("resource table should parse")
        .expect("resource should resolve");

        assert_eq!(sdk22_resource.value, ResourcePrimitive::Color(0xff11_1111));
        assert_eq!(sdk35_resource.value, ResourcePrimitive::Color(0xff22_2222));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::match
    #[test]
    fn resource_type_display_qualifiers_match_non_default_dimensions_and_orientation() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk(VALUE_TYPE_FIRST_COLOR_INT, 0xff00_0000),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0001,
                    res_table_display_config(ORIENTATION_PORT, 0, 600, 1000),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0002,
                    res_table_display_config(ORIENTATION_LAND, 0, 1000, 600),
                ),
            ],
        );

        let portrait = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android_with_display(35, 0, 600, 1000),
        )
        .expect("portrait resource table should parse")
        .expect("portrait resource should resolve");
        let landscape = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android_with_display(35, 0, 1000, 600),
        )
        .expect("landscape resource table should parse")
        .expect("landscape resource should resolve");
        let smaller = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android_with_display(35, 0, 599, 999),
        )
        .expect("smaller resource table should parse")
        .expect("default resource should resolve");

        assert_eq!(portrait.value, ResourcePrimitive::Color(0xff00_0001));
        assert_eq!(landscape.value, ResourcePrimitive::Color(0xff00_0002));
        assert_eq!(smaller.value, ResourcePrimitive::Color(0xff00_0000));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::isBetterThan
    #[test]
    fn resource_type_screen_size_qualifiers_compare_combined_width_height_delta() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk(VALUE_TYPE_FIRST_COLOR_INT, 0xff00_0000),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0001,
                    res_table_display_config(0, 0, 600, 600),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0002,
                    res_table_display_config(0, 0, 500, 950),
                ),
            ],
        );

        let resource = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android_with_display(35, 0, 600, 1000),
        )
        .expect("resource table should parse")
        .expect("resource should resolve");

        assert_eq!(resource.value, ResourcePrimitive::Color(0xff00_0002));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::isBetterThan
    #[test]
    fn resource_type_smallest_width_qualifier_precedes_screen_size_distance() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk(VALUE_TYPE_FIRST_COLOR_INT, 0xff00_0000),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0001,
                    res_table_display_config(0, 400, 600, 1000),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0002,
                    res_table_display_config(0, 500, 300, 500),
                ),
            ],
        );

        let resource = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android_with_display(35, 0, 600, 1000),
        )
        .expect("resource table should parse")
        .expect("resource should resolve");

        assert_eq!(resource.value, ResourcePrimitive::Color(0xff00_0002));
    }

    // cite: frameworks-base Configuration.java#reduceScreenLayout, setLocales, setLayoutDirection
    #[test]
    fn configuration_reduce_screen_layout_classifies_custom_dimensions() {
        assert_eq!(screen_layout_for_display(320, 460), 0x51);
        assert_eq!(screen_layout_for_display(420, 760), 0x1000_0062);
        assert_eq!(screen_layout_for_display(500, 700), 0x1000_0053);
        assert_eq!(screen_layout_for_display(720, 960), 0x1000_0054);
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::match, ResTable_config::isBetterThan
    #[test]
    fn resource_type_screen_layout_size_and_long_qualifiers_follow_display_configuration() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk(VALUE_TYPE_FIRST_COLOR_INT, 0xff00_0000),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0001,
                    res_table_screen_layout_config(0x11),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0002,
                    res_table_screen_layout_config(0x22),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0003,
                    res_table_screen_layout_config(0x13),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0004,
                    res_table_screen_layout_config(0x14),
                ),
            ],
        );

        let cases = [
            ((320, 460), ResourcePrimitive::Color(0xff00_0001)),
            ((420, 760), ResourcePrimitive::Color(0xff00_0002)),
            ((500, 700), ResourcePrimitive::Color(0xff00_0003)),
            ((720, 960), ResourcePrimitive::Color(0xff00_0004)),
        ];

        for ((width, height), expected) in cases {
            let resource = resolve_resource_value_for_config(
                &data,
                0x7f01_0000,
                ResourceResolutionConfig::android_with_display(35, 0, width, height),
            )
            .expect("resource table should parse")
            .expect("resource should resolve");
            assert_eq!(resource.value, expected);
        }
    }

    // cite: frameworks-base ResourcesImpl.java#updateConfiguration
    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::match, ResTable_config::isBetterThan
    #[test]
    fn resource_type_legacy_screen_size_qualifiers_follow_display_pixels() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk(VALUE_TYPE_FIRST_COLOR_INT, 0xff00_0000),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0001,
                    res_table_legacy_screen_config(800, 480),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0002,
                    res_table_legacy_screen_config(1000, 600),
                ),
            ],
        );

        let exact = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android_with_display(35, 0, 600, 1000),
        )
        .expect("resource table should parse")
        .expect("resource should resolve");
        let filtered = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android_with_display(35, 0, 600, 900),
        )
        .expect("resource table should parse")
        .expect("resource should resolve");

        assert_eq!(exact.value, ResourcePrimitive::Color(0xff00_0002));
        assert_eq!(filtered.value, ResourcePrimitive::Color(0xff00_0001));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::match, ResTable_config::isBetterThan
    #[test]
    fn resource_type_layout_direction_qualifier_requires_requested_direction() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk(VALUE_TYPE_FIRST_COLOR_INT, 0xff00_0000),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0001,
                    res_table_screen_layout_config(0x80),
                ),
            ],
        );

        let resource = resolve_resource_value_for_config(
            &data,
            0x7f01_0000,
            ResourceResolutionConfig::android_with_display(35, 0, 420, 760),
        )
        .expect("resource table should parse")
        .expect("resource should resolve");

        assert_eq!(resource.value, ResourcePrimitive::Color(0xff00_0000));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::isBetterThan
    #[test]
    fn resource_type_layout_direction_precedes_screen_width_qualifier() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0001,
                    res_table_display_config(0, 0, 420, 0),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0002,
                    res_table_screen_layout_config(0x40),
                ),
            ],
        );
        let requested = ResourceResolutionConfig::android_with_display(35, 0, 420, 760);

        let resource = resolve_resource_value_for_config(&data, 0x7f01_0000, requested)
            .expect("resource table should parse")
            .expect("resource should resolve");

        assert_eq!(resource.value, ResourcePrimitive::Color(0xff00_0002));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::match, ResTable_config::isBetterThan
    // cite: frameworks-base ResourceTypes.h#MASK_UI_MODE_TYPE, UI_MODE_TYPE_WATCH
    #[test]
    fn resource_type_watch_qualifier_requires_watch_ui_mode_type() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "color",
            "background",
            vec![
                type_chunk(VALUE_TYPE_FIRST_COLOR_INT, 0xffff_ffff),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff00_0000,
                    res_table_config(0, 0x06),
                ),
                type_chunk_with_config(
                    VALUE_TYPE_FIRST_COLOR_INT,
                    0xff11_1111,
                    res_table_config(0, 0x20),
                ),
            ],
        );
        let phone = ResourceResolutionConfig::android_with_display(35, 0x11, 420, 760);
        let watch = ResourceResolutionConfig::android_with_display(35, 0x16, 227, 227);
        let dark_watch = ResourceResolutionConfig::android_with_display(35, 0x26, 227, 227);

        for (requested, expected) in [
            (phone, 0xffff_ffff),
            (watch, 0xff00_0000),
            (dark_watch, 0xff00_0000),
        ] {
            let resource = resolve_resource_value_for_config(&data, 0x7f01_0000, requested)
                .expect("resource table should parse")
                .expect("resource should resolve");
            assert_eq!(resource.value, ResourcePrimitive::Color(expected));
        }
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::match, ResTable_config::isBetterThan
    // cite: frameworks-base ResourceTypes.h#screenLayout2, MASK_SCREENROUND
    #[test]
    fn resource_type_round_qualifier_follows_display_shape() {
        let data = minimal_resource_table_with_type_chunks(
            string_pool(&[]),
            "dimen",
            "inset",
            vec![
                type_chunk(VALUE_TYPE_INT_DEC, 1),
                type_chunk_with_config(VALUE_TYPE_INT_DEC, 2, res_table_screen_round_config(0x02)),
                type_chunk_with_config(VALUE_TYPE_INT_DEC, 3, res_table_screen_round_config(0x01)),
            ],
        );
        let display = ResourceResolutionConfig::android_with_display(35, 0x16, 227, 227);

        for (requested, expected) in [
            (display.with_screen_round(true), 2),
            (display.with_screen_round(false), 3),
            (ResourceResolutionConfig::android(35, 0x16), 1),
        ] {
            let resource = resolve_resource_value_for_config(&data, 0x7f01_0000, requested)
                .expect("resource table should parse")
                .expect("resource should resolve");
            assert_eq!(resource.value, ResourcePrimitive::Integer(expected));
        }
    }

    // cite: frameworks-base Configuration.java#SCREENLAYOUT_ROUND_YES, SCREENLAYOUT_ROUND_NO
    #[test]
    fn configuration_screen_layout_round_bits_match_framework_constants() {
        assert_eq!(configuration_screen_layout_round_bits(true), 0x200);
        assert_eq!(configuration_screen_layout_round_bits(false), 0x100);
    }

    fn minimal_resource_table(type_name: &str, key_name: &str, value: &str) -> Vec<u8> {
        let global_strings = string_pool(&[value]);
        minimal_resource_table_with_value(global_strings, type_name, key_name, VALUE_TYPE_STRING, 0)
    }

    fn minimal_typed_resource_table(
        type_name: &str,
        key_name: &str,
        value_type: u8,
        value: u32,
    ) -> Vec<u8> {
        let global_strings = string_pool(&[]);
        minimal_resource_table_with_value(global_strings, type_name, key_name, value_type, value)
    }

    fn minimal_array_resource_table(
        type_name: &str,
        key_name: &str,
        strings: &[&str],
        values: &[(u8, u32)],
    ) -> Vec<u8> {
        let global_strings = string_pool(strings);
        minimal_resource_table_with_type_chunk(
            global_strings,
            type_name,
            key_name,
            array_type_chunk(values),
        )
    }

    fn minimal_resource_table_with_value(
        global_strings: Vec<u8>,
        type_name: &str,
        key_name: &str,
        value_type: u8,
        value: u32,
    ) -> Vec<u8> {
        minimal_resource_table_with_type_chunk(
            global_strings,
            type_name,
            key_name,
            type_chunk(value_type, value),
        )
    }

    fn minimal_resource_table_with_type_chunk(
        global_strings: Vec<u8>,
        type_name: &str,
        key_name: &str,
        type_chunk: Vec<u8>,
    ) -> Vec<u8> {
        minimal_resource_table_with_type_chunks(
            global_strings,
            type_name,
            key_name,
            vec![type_chunk],
        )
    }

    fn minimal_resource_table_with_type_chunks(
        global_strings: Vec<u8>,
        type_name: &str,
        key_name: &str,
        type_chunks: Vec<Vec<u8>>,
    ) -> Vec<u8> {
        let type_strings = string_pool(&[type_name]);
        let key_strings = string_pool(&[key_name]);
        let type_strings_offset = 288u32;
        let key_strings_offset = type_strings_offset + type_strings.len() as u32;
        let type_chunks_size = type_chunks.iter().map(Vec::len).sum::<usize>();
        let package_size = 288 + type_strings.len() + key_strings.len() + type_chunks_size;
        let mut package = Vec::new();

        push_u16(&mut package, RES_TABLE_PACKAGE_TYPE);
        push_u16(&mut package, 288);
        push_u32(&mut package, package_size as u32);
        push_u32(&mut package, 0x7f);
        package.extend(vec![0; 256]);
        push_u32(&mut package, type_strings_offset);
        push_u32(&mut package, 0);
        push_u32(&mut package, key_strings_offset);
        push_u32(&mut package, 0);
        push_u32(&mut package, 0);
        package.extend(type_strings);
        package.extend(key_strings);
        for type_chunk in type_chunks {
            package.extend(type_chunk);
        }

        let table_size = 12 + global_strings.len() + package.len();
        let mut table = Vec::new();
        push_u16(&mut table, RES_TABLE_TYPE);
        push_u16(&mut table, 12);
        push_u32(&mut table, table_size as u32);
        push_u32(&mut table, 1);
        table.extend(global_strings);
        table.extend(package);
        table
    }

    fn type_chunk(value_type: u8, value: u32) -> Vec<u8> {
        type_chunk_with_optional_config(value_type, value, None)
    }

    fn type_chunk_with_config(value_type: u8, value: u32, config: Vec<u8>) -> Vec<u8> {
        type_chunk_with_optional_config(value_type, value, Some(config))
    }

    fn type_chunk_with_optional_config(
        value_type: u8,
        value: u32,
        config: Option<Vec<u8>>,
    ) -> Vec<u8> {
        let config = config.unwrap_or_default();
        let header_size = 20 + config.len();
        let entries_start = header_size + 4;
        let chunk_size = entries_start + 16;
        let mut chunk = Vec::new();
        push_u16(&mut chunk, RES_TABLE_TYPE_TYPE);
        push_u16(&mut chunk, header_size as u16);
        push_u32(&mut chunk, chunk_size as u32);
        chunk.push(0x01);
        chunk.push(0);
        push_u16(&mut chunk, 0);
        push_u32(&mut chunk, 1);
        push_u32(&mut chunk, entries_start as u32);
        chunk.extend(config);
        push_u32(&mut chunk, 0);
        push_u16(&mut chunk, 8);
        push_u16(&mut chunk, 0);
        push_u32(&mut chunk, 0);
        push_u16(&mut chunk, 8);
        chunk.push(0);
        chunk.push(value_type);
        push_u32(&mut chunk, value);
        chunk
    }

    fn array_type_chunk(values: &[(u8, u32)]) -> Vec<u8> {
        complex_type_chunk(None, values)
    }

    fn style_type_chunk(parent: Option<u32>, values: &[(u32, u8, u32)]) -> Vec<u8> {
        let values = values
            .iter()
            .map(|(name, value_type, value)| (*name, *value_type, *value))
            .collect::<Vec<_>>();
        complex_type_chunk_with_names(parent, &values)
    }

    fn complex_type_chunk(parent: Option<u32>, values: &[(u8, u32)]) -> Vec<u8> {
        let values = values
            .iter()
            .enumerate()
            .map(|(index, (value_type, value))| (index as u32, *value_type, *value))
            .collect::<Vec<_>>();
        complex_type_chunk_with_names(parent, &values)
    }

    fn complex_type_chunk_with_names(parent: Option<u32>, values: &[(u32, u8, u32)]) -> Vec<u8> {
        let entry_size = 16usize;
        let map_size = values.len() * 12;
        let chunk_size = 24 + entry_size + map_size;
        let mut chunk = Vec::new();
        push_u16(&mut chunk, RES_TABLE_TYPE_TYPE);
        push_u16(&mut chunk, 20);
        push_u32(&mut chunk, chunk_size as u32);
        chunk.push(0x01);
        chunk.push(0);
        push_u16(&mut chunk, 0);
        push_u32(&mut chunk, 1);
        push_u32(&mut chunk, 24);
        push_u32(&mut chunk, 0);
        push_u16(&mut chunk, entry_size as u16);
        push_u16(&mut chunk, ENTRY_FLAG_COMPLEX);
        push_u32(&mut chunk, 0);
        push_u32(&mut chunk, parent.unwrap_or(0));
        push_u32(&mut chunk, values.len() as u32);

        for (name, value_type, value) in values {
            push_u32(&mut chunk, *name);
            push_u16(&mut chunk, 8);
            chunk.push(0);
            chunk.push(*value_type);
            push_u32(&mut chunk, *value);
        }

        chunk
    }

    fn res_table_config(sdk_version: u16, ui_mode: u8) -> Vec<u8> {
        let mut config = vec![0; 32];
        write_u32(&mut config, 0, 32);
        write_u16(&mut config, 24, sdk_version);
        config[29] = ui_mode;
        config
    }

    fn res_table_display_config(
        orientation: u8,
        smallest_screen_width_dp: u16,
        screen_width_dp: u16,
        screen_height_dp: u16,
    ) -> Vec<u8> {
        let mut config = vec![0; 36];
        write_u32(&mut config, 0, 36);
        config[12] = orientation;
        write_u16(&mut config, 30, smallest_screen_width_dp);
        write_u16(&mut config, 32, screen_width_dp);
        write_u16(&mut config, 34, screen_height_dp);
        config
    }

    fn res_table_screen_layout_config(screen_layout: u8) -> Vec<u8> {
        let mut config = vec![0; 32];
        write_u32(&mut config, 0, 32);
        config[28] = screen_layout;
        config
    }

    fn res_table_screen_round_config(screen_layout2: u8) -> Vec<u8> {
        let mut config = vec![0; 52];
        write_u32(&mut config, 0, 52);
        config[48] = screen_layout2;
        config
    }

    fn res_table_legacy_screen_config(screen_width: u16, screen_height: u16) -> Vec<u8> {
        let mut config = vec![0; 24];
        write_u32(&mut config, 0, 24);
        write_u16(&mut config, 20, screen_width);
        write_u16(&mut config, 22, screen_height);
        config
    }

    fn string_pool(values: &[&str]) -> Vec<u8> {
        let mut string_data = Vec::new();
        let mut offsets = Vec::new();

        for value in values {
            offsets.push(string_data.len() as u32);
            push_utf8_string(&mut string_data, value);
        }

        let header_size = 28usize;
        let strings_start = header_size + values.len() * 4;
        let chunk_size = strings_start + string_data.len();
        let mut pool = Vec::new();

        push_u16(&mut pool, RES_STRING_POOL_TYPE);
        push_u16(&mut pool, header_size as u16);
        push_u32(&mut pool, chunk_size as u32);
        push_u32(&mut pool, values.len() as u32);
        push_u32(&mut pool, 0);
        push_u32(&mut pool, UTF8_FLAG);
        push_u32(&mut pool, strings_start as u32);
        push_u32(&mut pool, 0);

        for offset in offsets {
            push_u32(&mut pool, offset);
        }

        pool.extend(string_data);
        pool
    }

    fn push_utf8_string(data: &mut Vec<u8>, value: &str) {
        let length = value.len();
        assert!(length < 0x80);
        data.push(length as u8);
        data.push(length as u8);
        data.extend(value.as_bytes());
        data.push(0);
    }

    fn push_u16(data: &mut Vec<u8>, value: u16) {
        data.extend(value.to_le_bytes());
    }

    fn push_u32(data: &mut Vec<u8>, value: u32) {
        data.extend(value.to_le_bytes());
    }

    fn write_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }
}
