use super::*;
use crate::android_framework;
use crate::dex_run::DexDynamicViewGroup;
use crate::trace;

const MAX_THEME_ATTRIBUTE_REFERENCE_COUNT: usize = 20;
const MAX_RESOURCE_REFERENCE_COUNT: usize = 20;

thread_local! {
    static ARSC_CACHE: std::cell::RefCell<std::collections::HashMap<std::path::PathBuf, std::rc::Rc<Vec<u8>>>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

fn cached_arsc(apk_path: &Path, apk: &mut ApkArchive) -> Result<std::rc::Rc<Vec<u8>>> {
    if let Some(bytes) = ARSC_CACHE.with(|c| c.borrow().get(apk_path).cloned()) {
        return Ok(bytes);
    }
    let bytes = std::rc::Rc::new(apk.read_required_file("resources.arsc")?);
    ARSC_CACHE.with(|c| c.borrow_mut().insert(apk_path.to_owned(), bytes.clone()));
    Ok(bytes)
}

thread_local! {
    static ID_NAME_CACHE: std::cell::RefCell<
        std::collections::HashMap<
            std::path::PathBuf,
            std::rc::Rc<std::collections::BTreeMap<i32, String>>,
        >,
    > = std::cell::RefCell::new(std::collections::HashMap::new());
}

pub fn cached_resource_id_names(
    apk_path: &Path,
) -> Result<std::rc::Rc<std::collections::BTreeMap<i32, String>>> {
    if let Some(names) = ID_NAME_CACHE.with(|c| c.borrow().get(apk_path).cloned()) {
        return Ok(names);
    }
    let mut apk = ApkArchive::open(apk_path)?;
    let resources = cached_arsc(apk_path, &mut apk)?;
    let names = std::rc::Rc::new(
        crate::resource_table::resource_id_name_index(&resources)?
            .into_iter()
            .map(|(resource_id, name)| (resource_id as i32, name))
            .collect::<std::collections::BTreeMap<i32, String>>(),
    );
    ID_NAME_CACHE.with(|c| c.borrow_mut().insert(apk_path.to_owned(), names.clone()));
    Ok(names)
}

pub fn load_drawable_resource(
    apk_path: &Path,
    resource_id: u32,
    theme: &LayoutTheme,
) -> Result<LayoutDrawable> {
    let mut apk = ApkArchive::open(apk_path)?;
    let resources = cached_arsc(apk_path, &mut apk)?;

    load_drawable_resource_from_apk(&mut apk, &resources, resource_id, theme)
}

pub fn summarize_activity_layouts(
    apk_path: &Path,
    snapshot: &DexViewSnapshot,
    window_width: u32,
    window_height: u32,
) -> Result<LayoutRenderSummary> {
    let layout_ids = snapshot_layout_ids(snapshot);
    let theme = layout_theme_from_snapshot(snapshot);
    trace::count("perf", "layout render layout ids", layout_ids.len() as u64);

    if layout_ids.is_empty() {
        return Ok(LayoutRenderSummary {
            lines: vec!["layouts: none reached".to_owned()],
            text_elements: Vec::new(),
            view_visibilities: Vec::new(),
            scrollable_view_ids: BTreeSet::new(),
        });
    }

    let mut apk = {
        let _span = trace::span("render", "open APK for layout render");
        ApkArchive::open(apk_path)?
    };
    let resources = {
        let _span = trace::span("render", "read resources.arsc for layout render");
        cached_arsc(apk_path, &mut apk)?
    };
    let mut lines = Vec::new();
    let mut text_elements = Vec::new();
    let mut visibility_by_id = BTreeMap::new();
    let mut scrollable_view_ids = BTreeSet::new();
    let runtime_visibility_by_id = runtime_visibility_by_id(snapshot);
    let runtime_texts_by_id = runtime_texts_by_id(snapshot);

    let adapter_render = load_adapter_rows_render(&mut apk, &resources, &theme, snapshot)?;
    let adapter_ids: BTreeSet<u32> = adapter_render.keys().copied().collect();
    let adapter_item_layout_ids: BTreeSet<u32> = snapshot
        .adapter_rows
        .iter()
        .filter_map(|adapter| positive_resource_id(adapter.item_layout_id))
        .collect();
    let dynamic_children_by_id: BTreeMap<u32, &DexDynamicViewGroup> = snapshot
        .dynamic_children
        .iter()
        .map(|container| (container.container_view_id, container))
        .collect();
    let dynamic_children_ids: BTreeSet<u32> = dynamic_children_by_id.keys().copied().collect();

    for layout_id in layout_ids {
        if adapter_item_layout_ids.contains(&layout_id) {
            continue;
        }
        let resource = {
            let _span = trace::stat_span("perf", "layout resolve layout resource");
            resolve_layout_resource(&resources, layout_id, theme.config)?
        };
        let events = {
            let _span = trace::stat_span("perf", "layout read and parse XML events");
            read_layout_events(&mut apk, &resource)?
        };
        let mut tree = {
            let _span = trace::stat_span("perf", "layout load view tree");
            load_layout_tree(
                &mut apk,
                &resources,
                &resource,
                &theme,
                0,
                &mut BTreeSet::new(),
            )?
        };
        apply_runtime_visibility(&mut tree, &runtime_visibility_by_id);
        apply_runtime_texts(&mut tree, &runtime_texts_by_id);
        {
            let mut cache = LayoutResolutionCache::default();
            apply_theme_attribute_dimensions(&mut tree, &resources, &theme, &mut cache);
        }

        lines.push(format_layout_summary(&resource, &events));
        lines.extend(render_view_tree_lines(
            &tree,
            MAX_RENDERED_TREE_LINES_PER_LAYOUT,
        ));
        let mut layout_text_elements = Vec::new();
        collect_view_visibilities(&tree, &mut visibility_by_id);
        collect_scrollable_view_ids(&tree, false, &mut scrollable_view_ids);
        {
            let _span = trace::stat_span("perf", "layout collect text elements");
            collect_text_elements(
                &tree,
                0,
                LayoutFrame {
                    x: 0,
                    y: 0,
                    width: window_width,
                    height: window_height,
                },
                Some(&resources),
                Some(&theme),
                &mut layout_text_elements,
            )?;
        }
        if !adapter_render.is_empty() {
            let _span = trace::stat_span("perf", "layout expand adapter rows");
            let window_frame = LayoutFrame {
                x: 0,
                y: 0,
                width: window_width,
                height: window_height,
            };
            let mut adapter_frames = BTreeMap::new();
            collect_adapter_view_frames(&tree, window_frame, &adapter_ids, &mut adapter_frames);
            for (view_id, list_frame) in adapter_frames {
                let Some(adapter) = adapter_render.get(&view_id) else {
                    continue;
                };
                collect_adapter_rows_elements(
                    adapter,
                    list_frame,
                    window_height,
                    Some(&resources),
                    Some(&theme),
                    &mut layout_text_elements,
                )?;
            }
        }
        if !dynamic_children_by_id.is_empty() {
            let _span = trace::stat_span("perf", "layout expand dynamic addView children");
            let window_frame = LayoutFrame {
                x: 0,
                y: 0,
                width: window_width,
                height: window_height,
            };
            let mut dynamic_frames = BTreeMap::new();
            collect_adapter_view_frames(
                &tree,
                window_frame,
                &dynamic_children_ids,
                &mut dynamic_frames,
            );
            for (view_id, list_frame) in dynamic_frames {
                let Some(container) = dynamic_children_by_id.get(&view_id) else {
                    continue;
                };
                collect_dynamic_children_elements(
                    container,
                    list_frame,
                    window_height,
                    Some(&resources),
                    Some(&theme),
                    &mut layout_text_elements,
                )?;
            }
        }
        {
            let _span = trace::stat_span("perf", "layout attach drawables");
            attach_drawables(&mut apk, &resources, &theme, &mut layout_text_elements)?;
        }
        {
            // cite: frameworks-base styles_material.xml#TextAppearance.Material
            let mut cache = LayoutResolutionCache::default();
            if let Some(default_color) =
                resolve_theme_default_text_color(&mut apk, &resources, &theme, &mut cache)?
            {
                for element in &mut layout_text_elements {
                    if element.text_color.is_none() && !element.text.is_empty() {
                        element.text_color = Some(default_color);
                    }
                }
            }
        }
        lines.extend(render_text_frame_lines(&layout_text_elements));
        text_elements.extend(layout_text_elements);
    }

    Ok(LayoutRenderSummary {
        lines,
        text_elements,
        view_visibilities: visibility_by_id
            .into_iter()
            .map(|(view_id, visibility)| LayoutViewVisibility {
                view_id,
                visibility,
            })
            .collect(),
        scrollable_view_ids,
    })
}

fn collect_scrollable_view_ids(node: &ViewNode, in_scroll: bool, ids: &mut BTreeSet<u32>) {
    if in_scroll {
        if let Some(id) = node_id(node) {
            ids.insert(id);
        }
    }

    let child_in_scroll = in_scroll || is_vertical_scroll_container(node);
    for child in &node.children {
        collect_scrollable_view_ids(child, child_in_scroll, ids);
    }
}

fn is_vertical_scroll_container(node: &ViewNode) -> bool {
    node.name.ends_with("ScrollView") && !node.name.ends_with("HorizontalScrollView")
}

fn load_adapter_rows_render(
    apk: &mut ApkArchive,
    resources: &[u8],
    theme: &LayoutTheme,
    snapshot: &DexViewSnapshot,
) -> Result<BTreeMap<u32, AdapterRowsRender>> {
    let mut adapter_render = BTreeMap::new();

    for adapter in &snapshot.adapter_rows {
        let Ok(adapter_view_id) = u32::try_from(adapter.adapter_view_id) else {
            continue;
        };
        let Some(item_layout_id) = positive_resource_id(adapter.item_layout_id) else {
            continue;
        };
        let item_resource = resolve_layout_resource(resources, item_layout_id, theme.config)?;
        let mut item_tree = load_layout_tree(
            apk,
            resources,
            &item_resource,
            theme,
            0,
            &mut BTreeSet::new(),
        )?;
        {
            let mut cache = LayoutResolutionCache::default();
            apply_theme_attribute_dimensions(&mut item_tree, resources, theme, &mut cache);
        }

        let rows: Vec<AdapterRowRender> = adapter
            .rows
            .iter()
            .map(|row| AdapterRowRender {
                click_id: row.click_id as u32,
                texts: row
                    .texts
                    .iter()
                    .filter_map(|text| {
                        u32::try_from(text.view_id)
                            .ok()
                            .map(|view_id| (view_id, text.text.clone()))
                    })
                    .collect(),
                images: row
                    .images
                    .iter()
                    .filter_map(|image| {
                        u32::try_from(image.view_id)
                            .ok()
                            .map(|view_id| (view_id, image.drawable_resource_id))
                    })
                    .collect(),
                visibilities: row
                    .visibilities
                    .iter()
                    .filter_map(|visibility| {
                        u32::try_from(visibility.view_id)
                            .ok()
                            .map(|view_id| (view_id, visibility.visibility))
                    })
                    .collect(),
            })
            .collect();

        let image_intrinsic_sizes =
            adapter_row_image_intrinsic_sizes(apk, resources, theme, &rows)?;
        adapter_render.insert(
            adapter_view_id,
            AdapterRowsRender {
                item_tree,
                rows,
                image_intrinsic_sizes,
            },
        );
    }

    Ok(adapter_render)
}

fn adapter_row_image_intrinsic_sizes(
    apk: &mut ApkArchive,
    resources: &[u8],
    theme: &LayoutTheme,
    rows: &[AdapterRowRender],
) -> Result<BTreeMap<u32, (u32, u32)>> {
    let mut sizes = BTreeMap::new();
    for row in rows {
        for &resource_id in row.images.values() {
            if let std::collections::btree_map::Entry::Vacant(entry) = sizes.entry(resource_id) {
                if let Some(size) =
                    drawable_intrinsic_pixel_size(apk, resources, resource_id, theme)?
                {
                    entry.insert(size);
                }
            }
        }
    }
    Ok(sizes)
}

pub(crate) fn resolve_theme_color_attribute(
    apk_path: &Path,
    snapshot: &DexViewSnapshot,
    name: &str,
) -> Result<Option<u32>> {
    let mut apk = ApkArchive::open(apk_path)?;
    let resources = cached_arsc(apk_path, &mut apk)?;
    let theme = layout_theme_from_snapshot(snapshot);
    let mut cache = LayoutResolutionCache::default();

    let Some(attribute_id) = layout_attr_resource_id_cached(&resources, &mut cache, name)? else {
        return Ok(None);
    };

    Ok(resolve_layout_theme_color_attribute_cached(
        &resources,
        Some(&theme),
        &mut cache,
        attribute_id,
    )
    .ok())
}

// cite: frameworks-base themes_material.xml#textAppearance
// cite: frameworks-base styles_material.xml#TextAppearance.Material
// cite: frameworks-base public-final.xml#textColorPrimary
pub(crate) fn resolve_theme_default_text_color(
    apk: &mut ApkArchive,
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
) -> Result<Option<u32>> {
    const ANDROID_ATTR_TEXT_COLOR_PRIMARY: u32 = 0x0101_0036;
    if let Some(primitive) = resolve_layout_theme_attribute_cached(
        resources,
        theme,
        cache,
        ANDROID_ATTR_TEXT_COLOR_PRIMARY,
    ) {
        if let Some(color) =
            resolve_color_primitive_with_state_lists(apk, resources, theme, cache, primitive)?
        {
            return Ok(Some(color));
        }
    }

    Ok(theme.styles.iter().find_map(|style| {
        theme_parent_chain(resources, theme, cache, style.resource_id)
            .into_iter()
            .find_map(android_framework::framework_theme_text_color_primary)
    }))
}

fn resolve_color_primitive_with_state_lists(
    apk: &mut ApkArchive,
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    primitive: resource_table::ResourcePrimitive,
) -> Result<Option<u32>> {
    let mut visited_attributes = BTreeSet::new();
    resolve_color_primitive_with_state_lists_inner(
        apk,
        resources,
        theme,
        cache,
        primitive,
        0,
        &mut visited_attributes,
    )
}

fn resolve_color_primitive_with_state_lists_inner(
    apk: &mut ApkArchive,
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    primitive: resource_table::ResourcePrimitive,
    attribute_reference_count: usize,
    visited_attributes: &mut BTreeSet<u32>,
) -> Result<Option<u32>> {
    match primitive {
        resource_table::ResourcePrimitive::Color(value)
        | resource_table::ResourcePrimitive::Hex(value) => Ok(Some(value)),
        resource_table::ResourcePrimitive::Integer(value) => Ok(Some(value as u32)),
        // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
        resource_table::ResourcePrimitive::Reference(0) => Ok(None),
        resource_table::ResourcePrimitive::Reference(resource_id) => {
            let primitive =
                resolve_layout_color_resource_primitive(resources, resource_id, theme.config)?;
            resolve_color_primitive_with_state_lists_inner(
                apk,
                resources,
                theme,
                cache,
                primitive,
                attribute_reference_count,
                visited_attributes,
            )
        }
        resource_table::ResourcePrimitive::Attribute(attribute_id) => {
            // cite: frameworks-base ResourceTypes.cpp#ResTable::Theme::getAttribute
            if attribute_reference_count == MAX_THEME_ATTRIBUTE_REFERENCE_COUNT {
                return Err(WinedroidError::new(format!(
                    "layout color state-list attribute 0x{attribute_id:08x} exceeded {MAX_THEME_ATTRIBUTE_REFERENCE_COUNT} references"
                )));
            }
            if !visited_attributes.insert(attribute_id) {
                return Err(WinedroidError::new(format!(
                    "layout color state-list attribute reference cycle reached 0x{attribute_id:08x}"
                )));
            }
            let next = resolve_layout_theme_attribute_cached(resources, theme, cache, attribute_id)
                .ok_or_else(|| {
                    WinedroidError::new(format!(
                        "layout color state-list attribute 0x{attribute_id:08x} was not found in active theme"
                    ))
                })?;
            resolve_color_primitive_with_state_lists_inner(
                apk,
                resources,
                theme,
                cache,
                next,
                attribute_reference_count + 1,
                visited_attributes,
            )
        }
        resource_table::ResourcePrimitive::String(path) if path.ends_with(".xml") => {
            color_state_list_default_color(apk, resources, theme, cache, &path)
        }
        _ => Ok(None),
    }
}

#[cfg(test)]
pub(crate) fn test_resolve_color_primitive_with_state_lists(
    apk: &mut ApkArchive,
    resources: &[u8],
    theme: &LayoutTheme,
    primitive: resource_table::ResourcePrimitive,
) -> Result<Option<u32>> {
    let mut cache = LayoutResolutionCache::default();
    resolve_color_primitive_with_state_lists(apk, resources, theme, &mut cache, primitive)
}

// cite: frameworks-base ColorStateList.java#inflate, onColorsChanged
fn color_state_list_default_color(
    apk: &mut ApkArchive,
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    path: &str,
) -> Result<Option<u32>> {
    let data = apk.read_required_file(path)?;
    let events = crate::binary_xml::parse_binary_xml(&data)?;

    let mut in_selector = false;
    let mut items: Vec<(bool, XmlValue)> = Vec::new();
    for event in &events {
        match event {
            XmlEvent::Start(element) if element.name == "selector" => {
                in_selector = true;
            }
            XmlEvent::Start(element) if element.name == "item" && in_selector => {
                let has_states = element
                    .attributes
                    .iter()
                    .any(|attribute| attribute.name.starts_with("state_"));
                if let Some(color) = element.attribute("color") {
                    items.push((has_states, color.value.clone()));
                }
            }
            _ => {}
        }
    }

    if items.is_empty() {
        return Ok(None);
    }

    let mut chosen = &items[0].1;
    for (has_states, value) in items.iter().skip(1).rev() {
        if !has_states {
            chosen = value;
            break;
        }
    }

    resolve_xml_color_value_cached(resources, Some(theme), cache, &chosen.clone())
}

// cite: androidx AppCompatDelegateImpl.java#createSubDecor
// cite: frameworks-base PhoneWindow.java#generateLayout
// cite: frameworks-base themes.xml#windowActionBar, windowNoTitle
pub fn activity_has_decor_action_bar(apk_path: &Path, snapshot: &DexViewSnapshot) -> bool {
    let Ok(mut apk) = ApkArchive::open(apk_path) else {
        return false;
    };
    let Ok(resources) = cached_arsc(apk_path, &mut apk) else {
        return false;
    };

    activity_has_decor_action_bar_from_resources(&resources, snapshot)
}

pub fn activity_has_decor_action_bar_from_resources(
    resources: &[u8],
    snapshot: &DexViewSnapshot,
) -> bool {
    let theme = layout_theme_from_snapshot(snapshot);
    let mut cache = LayoutResolutionCache::default();

    let framework_default = snapshot.theme_styles.iter().find_map(|style| {
        theme_parent_chain(resources, &theme, &mut cache, style.resource_id)
            .into_iter()
            .find_map(crate::android_framework::framework_theme_has_decor_action_bar)
    });

    let window_action_bar = theme_bool_attribute(resources, &theme, &mut cache, "windowActionBar")
        .or(framework_default)
        .unwrap_or(false);
    let window_no_title =
        theme_bool_attribute(resources, &theme, &mut cache, "windowNoTitle").unwrap_or(false);

    window_action_bar && !window_no_title
}

fn theme_bool_attribute(
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    name: &str,
) -> Option<bool> {
    let attribute_id = layout_attr_resource_id_cached(resources, cache, name).ok()??;
    match resolve_layout_theme_attribute_cached(resources, theme, cache, attribute_id)? {
        resource_table::ResourcePrimitive::Boolean(value) => Some(value),
        resource_table::ResourcePrimitive::Integer(value) => Some(value != 0),
        _ => None,
    }
}

// cite: frameworks-base PhoneWindow.java#Window_windowBackground
// cite: androidx themes_base.xml#windowBackground
pub fn activity_window_background_color(
    apk_path: &Path,
    snapshot: &DexViewSnapshot,
) -> Option<u32> {
    let mut apk = ApkArchive::open(apk_path).ok()?;
    let resources = cached_arsc(apk_path, &mut apk).ok()?;

    activity_window_background_color_from_resources(&resources, snapshot)
}

pub fn activity_window_background_color_from_resources(
    resources: &[u8],
    snapshot: &DexViewSnapshot,
) -> Option<u32> {
    let theme = layout_theme_from_snapshot(snapshot);
    let mut cache = LayoutResolutionCache::default();
    let attribute_id = android_framework_attr_resource_id("windowBackground")?;
    if let Some(primitive) =
        resolve_layout_theme_attribute_cached(resources, &theme, &mut cache, attribute_id)
    {
        if let Ok(color) = resolve_layout_color_primitive_cached(
            resources,
            &theme,
            &mut cache,
            attribute_id,
            primitive,
            0,
        ) {
            return Some(color);
        }
    }

    theme.styles.iter().find_map(|style| {
        theme_parent_chain(resources, &theme, &mut cache, style.resource_id)
            .into_iter()
            .find_map(crate::android_framework::framework_theme_window_background)
    })
}

pub fn activity_is_floating(apk_path: &Path, snapshot: &DexViewSnapshot) -> bool {
    let Ok(mut apk) = ApkArchive::open(apk_path) else {
        return false;
    };
    let Ok(resources) = cached_arsc(apk_path, &mut apk) else {
        return false;
    };

    activity_is_floating_from_resources(&resources, snapshot)
}

pub fn activity_is_floating_from_resources(resources: &[u8], snapshot: &DexViewSnapshot) -> bool {
    let theme = layout_theme_from_snapshot(snapshot);
    let mut cache = LayoutResolutionCache::default();

    snapshot.theme_styles.iter().any(|style| {
        theme_parent_chain(resources, &theme, &mut cache, style.resource_id)
            .into_iter()
            .any(crate::android_framework::is_floating_window_theme)
    })
}

fn theme_parent_chain(
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    style_id: u32,
) -> Vec<u32> {
    let mut chain = Vec::new();
    let mut current = style_id;
    for _ in 0..24 {
        if current == 0 || chain.contains(&current) {
            break;
        }
        chain.push(current);
        let Ok(Some(resource)) =
            resolve_layout_resource_value_cached(resources, Some(theme), cache, current)
        else {
            break;
        };
        match resource.value {
            resource_table::ResourcePrimitive::Bag(bag) => match bag.parent {
                Some(parent) => current = parent,
                None => break,
            },
            _ => break,
        }
    }
    chain
}

pub(crate) fn layout_theme_from_snapshot(snapshot: &DexViewSnapshot) -> LayoutTheme {
    LayoutTheme {
        styles: snapshot
            .theme_styles
            .iter()
            .map(|style| LayoutThemeStyle {
                resource_id: style.resource_id,
                force: style.force,
            })
            .collect(),
        config: layout_resource_config_from_snapshot(snapshot),
    }
}

pub(crate) fn layout_resource_config_from_snapshot(
    snapshot: &DexViewSnapshot,
) -> resource_table::ResourceResolutionConfig {
    resource_table::ResourceResolutionConfig::android_with_display(
        snapshot.resource_sdk_version,
        snapshot.resource_ui_mode,
        snapshot.resource_display_width,
        snapshot.resource_display_height,
    )
    .with_screen_round(snapshot.resource_screen_round)
}

pub(crate) fn runtime_texts_by_id(snapshot: &DexViewSnapshot) -> BTreeMap<u32, String> {
    snapshot
        .view_texts
        .iter()
        .filter_map(|text| {
            let view_id = u32::try_from(text.view_id).ok()?;
            Some((view_id, text.text.clone()))
        })
        .collect()
}

pub(crate) fn runtime_visibility_by_id(snapshot: &DexViewSnapshot) -> BTreeMap<u32, i32> {
    snapshot
        .view_visibilities
        .iter()
        .filter_map(|visibility| {
            let view_id = u32::try_from(visibility.view_id).ok()?;
            Some((view_id, visibility.visibility))
        })
        .collect()
}

pub(crate) fn snapshot_layout_ids(snapshot: &DexViewSnapshot) -> Vec<u32> {
    let mut ids = BTreeSet::new();

    for value in &snapshot.content_layouts {
        if let Some(id) = positive_resource_id(*value) {
            ids.insert(id);
        }
    }

    for value in &snapshot.inflated_layouts {
        if let Some(id) = positive_resource_id(*value) {
            ids.insert(id);
        }
    }

    ids.into_iter().collect()
}

pub(crate) fn positive_resource_id(value: i32) -> Option<u32> {
    if value <= 0 {
        return None;
    }

    Some(value as u32)
}

pub(crate) fn resolve_layout_resource(
    resources: &[u8],
    layout_id: u32,
    config: resource_table::ResourceResolutionConfig,
) -> Result<ResourceFile> {
    let Some(resource) =
        resource_table::resolve_resource_file_for_config(resources, layout_id, config)?
    else {
        return Err(WinedroidError::new(format!(
            "layout resource 0x{layout_id:08x} was not found in resources.arsc"
        )));
    };

    if resource.type_name != "layout" {
        return Err(WinedroidError::new(format!(
            "resource 0x{layout_id:08x} resolved to type `{}`, expected `layout`",
            resource.type_name
        )));
    }

    Ok(resource)
}

pub(crate) fn read_layout_events(
    apk: &mut ApkArchive,
    resource: &ResourceFile,
) -> Result<Vec<XmlEvent>> {
    let data = apk.read_required_file(&resource.path)?;

    binary_xml::parse_binary_xml(&data).map_err(|error| {
        WinedroidError::with_source(
            format!(
                "failed to parse layout XML resource 0x{:08x} at `{}`",
                resource.resource_id, resource.path
            ),
            error,
        )
    })
}

pub(crate) fn attach_drawables(
    apk: &mut ApkArchive,
    resources: &[u8],
    theme: &LayoutTheme,
    elements: &mut [LayoutTextElement],
) -> Result<()> {
    for element in elements {
        let Some(resource_id) = element.drawable_resource_id else {
            continue;
        };
        let mut drawable = load_drawable_resource_from_apk(apk, resources, resource_id, theme)?;
        drawable.tint_color =
            resolve_layout_drawable_tint(resources, Some(theme), element.drawable_tint)?;
        element.drawable = Some(drawable);
    }

    Ok(())
}

pub(crate) fn attach_image_intrinsic_sizes(
    apk: &mut ApkArchive,
    resources: &[u8],
    theme: &LayoutTheme,
    node: &mut ViewNode,
) -> Result<()> {
    if node.name.ends_with("ImageView") || node.name.ends_with("ImageButton") {
        if node_intrinsic_image_size(node).is_none() {
            if let Some(resource_id) =
                image_drawable_resource_id(node, Some(resources), theme.config)?
            {
                if let Some((width, height)) =
                    drawable_intrinsic_pixel_size(apk, resources, resource_id, theme)?
                {
                    push_intrinsic_size_attributes(node, width, height);
                }
            }
        }
    }

    for child in &mut node.children {
        attach_image_intrinsic_sizes(apk, resources, theme, child)?;
    }

    Ok(())
}

fn push_intrinsic_size_attributes(node: &mut ViewNode, width: u32, height: u32) {
    for (name, value) in [
        (INTRINSIC_IMAGE_WIDTH_ATTR, width),
        (INTRINSIC_IMAGE_HEIGHT_ATTR, height),
    ] {
        node.attributes.push(ViewAttribute {
            name: name.to_owned(),
            value: value.to_string(),
            typed_value: XmlValue::Integer(value as i32),
            reference: None,
        });
    }
}

fn drawable_intrinsic_pixel_size(
    apk: &mut ApkArchive,
    resources: &[u8],
    resource_id: u32,
    theme: &LayoutTheme,
) -> Result<Option<(u32, u32)>> {
    let drawable = load_drawable_resource_from_apk(apk, resources, resource_id, theme)?;
    Ok(png_pixel_size(&drawable.data))
}

fn png_pixel_size(data: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if data.len() < 24 || data[0..8] != PNG_SIGNATURE {
        return None;
    }
    let width = u32::from_be_bytes([data[16], data[17], data[18], data[19]]);
    let height = u32::from_be_bytes([data[20], data[21], data[22], data[23]]);
    (width > 0 && height > 0).then_some((width, height))
}

pub(crate) fn load_drawable_resource_from_apk(
    apk: &mut ApkArchive,
    resources: &[u8],
    resource_id: u32,
    theme: &LayoutTheme,
) -> Result<LayoutDrawable> {
    let Some(resource) =
        resource_table::resolve_resource_file_for_config(resources, resource_id, theme.config)?
    else {
        return Err(WinedroidError::new(format!(
            "drawable resource 0x{resource_id:08x} was not found in resources.arsc"
        )));
    };
    if resource.type_name != "drawable" && resource.type_name != "mipmap" {
        return Err(WinedroidError::new(format!(
            "resource 0x{resource_id:08x} resolved to type `{}`, expected `drawable` or `mipmap`",
            resource.type_name
        )));
    }

    let data = apk.read_required_file(&resource.path)?;
    Ok(LayoutDrawable {
        resource_id,
        path: resource.path,
        data,
        resources: resources.to_vec(),
        theme: theme.clone(),
        tint_color: None,
    })
}

pub(crate) fn resolve_layout_drawable_tint(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    tint: Option<LayoutDrawableTint>,
) -> Result<Option<u32>> {
    let Some(tint) = tint else {
        return Ok(None);
    };

    match tint {
        LayoutDrawableTint::Color(value) => Ok(Some(value)),
        LayoutDrawableTint::Resource(resource_id) => {
            resolve_layout_color_resource(resources, theme, resource_id).map(Some)
        }
        LayoutDrawableTint::Attribute(attribute_id) => {
            resolve_layout_theme_color_attribute(resources, theme, attribute_id).map(Some)
        }
    }
}

pub(crate) fn resolve_layout_color_resource(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    resource_id: u32,
) -> Result<u32> {
    let mut cache = LayoutResolutionCache::default();
    resolve_layout_color_resource_cached(resources, theme, &mut cache, resource_id)
}

#[derive(Default)]
pub(crate) struct LayoutResolutionCache {
    attr_resource_ids: BTreeMap<String, Option<u32>>,
    resource_values: BTreeMap<u32, Option<resource_table::ResourceValue>>,
    style_attributes: BTreeMap<(u32, u32), Option<resource_table::ResourcePrimitive>>,
}

pub(crate) fn resolve_layout_color_resource_cached(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    resource_id: u32,
) -> Result<u32> {
    resolve_layout_color_resource_inner(resources, theme, cache, resource_id)
}

pub(crate) fn resolve_layout_dimension_resource_inner(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    resource_id: u32,
) -> Result<i32> {
    let config = theme.map(|theme| theme.config).unwrap_or_default();
    let resource = resource_table::resolve_resource_value_references_for_config(
        resources,
        resource_id,
        config,
    )?
    .ok_or_else(|| {
        WinedroidError::new(format!(
            "layout dimension resource 0x{resource_id:08x} was not found"
        ))
    })?;

    match resource.value {
        resource_table::ResourcePrimitive::Dimension(value) => Ok(complex_dimension_to_px(value)),
        resource_table::ResourcePrimitive::Integer(value) => Ok(value),
        resource_table::ResourcePrimitive::Attribute(attribute_id) => {
            resolve_layout_theme_dimension_attribute_cached(resources, theme, cache, attribute_id)
        }
        value => Err(WinedroidError::new(format!(
            "layout dimension resource 0x{resource_id:08x} has unsupported value {value:?}"
        ))),
    }
}

pub(crate) fn resolve_layout_theme_dimension_attribute_cached(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    attribute_id: u32,
) -> Result<i32> {
    if let Some(value) = android_framework::framework_attr_dimension_px(attribute_id) {
        return Ok(value);
    }

    let Some(theme) = theme else {
        return Err(WinedroidError::new(format!(
            "layout dimension attribute 0x{attribute_id:08x} needs an active theme"
        )));
    };
    let Some(primitive) =
        resolve_layout_theme_attribute_cached(resources, theme, cache, attribute_id)
    else {
        return Err(WinedroidError::new(format!(
            "layout dimension attribute 0x{attribute_id:08x} was not found in active theme"
        )));
    };

    resolve_layout_dimension_primitive_cached(resources, theme, cache, attribute_id, primitive)
}

pub(crate) fn resolve_layout_dimension_primitive_cached(
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    attribute_id: u32,
    primitive: resource_table::ResourcePrimitive,
) -> Result<i32> {
    let mut visited_attributes = BTreeSet::from([attribute_id]);
    resolve_layout_dimension_primitive_inner(
        resources,
        theme,
        cache,
        attribute_id,
        primitive,
        0,
        &mut visited_attributes,
    )
}

fn resolve_layout_dimension_primitive_inner(
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    origin_attribute_id: u32,
    primitive: resource_table::ResourcePrimitive,
    attribute_reference_count: usize,
    visited_attributes: &mut BTreeSet<u32>,
) -> Result<i32> {
    match primitive {
        resource_table::ResourcePrimitive::Dimension(value) => Ok(complex_dimension_to_px(value)),
        resource_table::ResourcePrimitive::Integer(value) => Ok(value),
        resource_table::ResourcePrimitive::Reference(resource_id) => {
            resolve_layout_dimension_resource_inner(resources, Some(theme), cache, resource_id)
        }
        resource_table::ResourcePrimitive::Attribute(next_attribute_id) => {
            if let Some(pixels) = android_framework::framework_attr_dimension_px(next_attribute_id)
            {
                return Ok(pixels);
            }
            // cite: frameworks-base ResourceTypes.cpp#ResTable::Theme::getAttribute
            if attribute_reference_count == MAX_THEME_ATTRIBUTE_REFERENCE_COUNT {
                return Err(WinedroidError::new(format!(
                    "layout dimension attribute 0x{origin_attribute_id:08x} exceeded {MAX_THEME_ATTRIBUTE_REFERENCE_COUNT} references"
                )));
            }
            if !visited_attributes.insert(next_attribute_id) {
                return Err(WinedroidError::new(format!(
                    "layout dimension attribute reference cycle reached 0x{next_attribute_id:08x} from 0x{origin_attribute_id:08x}"
                )));
            }
            let value = resolve_layout_theme_attribute_cached(resources, theme, cache, next_attribute_id)
                .ok_or_else(|| {
                    WinedroidError::new(format!(
                        "layout dimension attribute 0x{origin_attribute_id:08x} references missing attribute 0x{next_attribute_id:08x}"
                    ))
                })?;
            resolve_layout_dimension_primitive_inner(
                resources,
                theme,
                cache,
                origin_attribute_id,
                value,
                attribute_reference_count + 1,
                visited_attributes,
            )
        }
        value => Err(WinedroidError::new(format!(
            "layout dimension attribute 0x{origin_attribute_id:08x} has unsupported value {value:?}"
        ))),
    }
}

fn resolve_layout_color_resource_inner(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    resource_id: u32,
) -> Result<u32> {
    let config = theme.map(|theme| theme.config).unwrap_or_default();
    match resolve_layout_color_resource_primitive(resources, resource_id, config)? {
        resource_table::ResourcePrimitive::Color(value)
        | resource_table::ResourcePrimitive::Hex(value) => Ok(value),
        resource_table::ResourcePrimitive::Integer(value) => Ok(value as u32),
        resource_table::ResourcePrimitive::Attribute(attribute_id) => {
            resolve_layout_theme_color_attribute_cached(resources, theme, cache, attribute_id)
        }
        value => Err(WinedroidError::new(format!(
            "layout color resource 0x{resource_id:08x} has unsupported value {value:?}"
        ))),
    }
}

pub(crate) fn resolve_layout_resource_value(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    resource_id: u32,
) -> Result<Option<resource_table::ResourceValue>> {
    if let Some(theme) = theme {
        return resource_table::resolve_resource_value_for_config(
            resources,
            resource_id,
            theme.config,
        );
    }

    resource_table::resolve_resource_value(resources, resource_id)
}

pub(crate) fn resolve_layout_resource_value_cached(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    resource_id: u32,
) -> Result<Option<resource_table::ResourceValue>> {
    if let Some(value) = cache.resource_values.get(&resource_id) {
        return Ok(value.clone());
    }

    let value = resolve_layout_resource_value(resources, theme, resource_id)?;
    cache.resource_values.insert(resource_id, value.clone());

    Ok(value)
}

pub(crate) fn resolve_layout_theme_color_attribute(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    attribute_id: u32,
) -> Result<u32> {
    let mut cache = LayoutResolutionCache::default();
    resolve_layout_theme_color_attribute_cached(resources, theme, &mut cache, attribute_id)
}

pub(crate) fn resolve_layout_theme_color_attribute_cached(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    attribute_id: u32,
) -> Result<u32> {
    let Some(theme) = theme else {
        return Err(WinedroidError::new(format!(
            "layout color attribute 0x{attribute_id:08x} needs an active theme"
        )));
    };
    let primitive = resolve_layout_theme_attribute_cached(resources, theme, cache, attribute_id)
        .ok_or_else(|| {
            WinedroidError::new(format!(
                "layout color attribute 0x{attribute_id:08x} was not found in active theme"
            ))
        })?;

    resolve_layout_color_primitive_cached(resources, theme, cache, attribute_id, primitive, 0)
}

pub(crate) fn resolve_layout_color_primitive_cached(
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    attribute_id: u32,
    primitive: resource_table::ResourcePrimitive,
    depth: usize,
) -> Result<u32> {
    // cite: frameworks-base ResourceTypes.cpp#ResTable::Theme::getAttribute
    if depth > MAX_THEME_ATTRIBUTE_REFERENCE_COUNT {
        return Err(WinedroidError::new(format!(
            "layout color attribute 0x{attribute_id:08x} reference depth exceeded"
        )));
    }

    match primitive {
        resource_table::ResourcePrimitive::Color(value)
        | resource_table::ResourcePrimitive::Hex(value) => Ok(value),
        resource_table::ResourcePrimitive::Integer(value) => Ok(value as u32),
        resource_table::ResourcePrimitive::Reference(resource_id) => {
            resolve_layout_color_resource_inner(resources, Some(theme), cache, resource_id)
        }
        resource_table::ResourcePrimitive::Attribute(next_attribute_id) => {
            let value = resolve_layout_theme_attribute_cached(resources, theme, cache, next_attribute_id)
                .ok_or_else(|| {
                    WinedroidError::new(format!(
                        "layout color attribute 0x{attribute_id:08x} references missing attribute 0x{next_attribute_id:08x}"
                    ))
                })?;
            resolve_layout_color_primitive_cached(
                resources,
                theme,
                cache,
                next_attribute_id,
                value,
                depth + 1,
            )
        }
        value => Err(WinedroidError::new(format!(
            "layout color attribute 0x{attribute_id:08x} has unsupported value {value:?}"
        ))),
    }
}

fn resolve_layout_color_resource_primitive(
    resources: &[u8],
    resource_id: u32,
    config: resource_table::ResourceResolutionConfig,
) -> Result<resource_table::ResourcePrimitive> {
    let mut current_id = resource_id;
    let mut reference_count = 0;
    let mut visited = BTreeSet::new();

    loop {
        if !visited.insert(current_id) {
            return Err(WinedroidError::new(format!(
                "layout color resource reference cycle reached 0x{current_id:08x} from 0x{resource_id:08x}"
            )));
        }
        if let Some(value) = android_framework::color_resource(current_id) {
            return Ok(resource_table::ResourcePrimitive::Color(value));
        }

        // cite: frameworks-base ResourceTypes.cpp#ResTable::getResource, ResTable::resolveReference
        let Some(resource) =
            resource_table::resolve_resource_value_for_config(resources, current_id, config)?
        else {
            if reference_count == 0 {
                return Err(WinedroidError::new(format!(
                    "layout color resource 0x{resource_id:08x} was not found"
                )));
            }
            return Err(WinedroidError::new(format!(
                "layout color resource 0x{resource_id:08x} references missing resource 0x{current_id:08x}"
            )));
        };

        let next_id = match &resource.value {
            resource_table::ResourcePrimitive::Reference(next_id) => *next_id,
            _ => return Ok(resource.value),
        };
        if next_id == 0 {
            return Ok(resource.value);
        }
        if reference_count == MAX_RESOURCE_REFERENCE_COUNT {
            return Err(WinedroidError::new(format!(
                "layout color resource 0x{resource_id:08x} exceeded {MAX_RESOURCE_REFERENCE_COUNT} references"
            )));
        }

        reference_count += 1;
        current_id = next_id;
    }
}

pub(crate) fn resolve_layout_theme_attribute_cached(
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    attribute_id: u32,
) -> Option<resource_table::ResourcePrimitive> {
    let mut resolved = None;

    for style in &theme.styles {
        let value = resolve_layout_style_attribute_cached(
            resources,
            theme,
            cache,
            style.resource_id,
            attribute_id,
            &mut BTreeSet::new(),
        );
        if let Some(value) = value {
            if style.force || resolved.is_none() {
                resolved = Some(value);
            }
        }
    }

    resolved
}

pub(crate) fn resolve_layout_style_attribute_cached(
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    style_resource_id: u32,
    attribute_id: u32,
    visited: &mut BTreeSet<u32>,
) -> Option<resource_table::ResourcePrimitive> {
    if let Some(value) = cache
        .style_attributes
        .get(&(style_resource_id, attribute_id))
        .cloned()
    {
        return value;
    }

    if !visited.insert(style_resource_id) {
        return None;
    }

    let resource =
        resolve_layout_resource_value_cached(resources, Some(theme), cache, style_resource_id)
            .ok()
            .flatten()?;
    let resource_table::ResourcePrimitive::Bag(bag) = resource.value else {
        cache
            .style_attributes
            .insert((style_resource_id, attribute_id), None);
        return None;
    };

    let mut resolved = bag.parent.and_then(|parent| {
        resolve_layout_style_attribute_cached(
            resources,
            theme,
            cache,
            parent,
            attribute_id,
            visited,
        )
    });
    for entry in bag.entries {
        if entry.name == attribute_id {
            resolved = Some(entry.value);
        }
    }

    cache
        .style_attributes
        .insert((style_resource_id, attribute_id), resolved.clone());

    resolved
}

#[cfg(test)]
pub(crate) fn resolve_style_color_attribute(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    style_resource_id: u32,
    attribute_name: &str,
) -> Result<Option<u32>> {
    let mut cache = LayoutResolutionCache::default();
    resolve_style_color_attribute_cached(
        resources,
        theme,
        &mut cache,
        style_resource_id,
        attribute_name,
        false,
    )
}

pub(crate) fn resolve_style_color_attribute_cached(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    style_resource_id: u32,
    attribute_name: &str,
    paint_drawable_background_tint: bool,
) -> Result<Option<u32>> {
    let Some(theme) = theme else {
        return Ok(None);
    };
    let Some(attribute_id) = layout_attr_resource_id_cached(resources, cache, attribute_name)?
    else {
        return Ok(None);
    };
    let Some(value) = resolve_layout_style_attribute_cached(
        resources,
        theme,
        cache,
        style_resource_id,
        attribute_id,
        &mut BTreeSet::new(),
    ) else {
        return Ok(None);
    };

    if attribute_name == "backgroundTint"
        && !paint_drawable_background_tint
        && style_has_drawable_background(resources, Some(theme), cache, style_resource_id)?
    {
        return Ok(None);
    }

    if style_color_attribute_can_reference_drawable(attribute_name)
        && primitive_is_drawable_reference(resources, theme, cache, &value)?
    {
        return Ok(None);
    }

    resolve_layout_color_primitive_cached(resources, theme, cache, attribute_id, value, 0).map(Some)
}

fn style_color_attribute_can_reference_drawable(attribute_name: &str) -> bool {
    matches!(attribute_name, "background" | "foreground")
}

pub(crate) fn style_has_drawable_background(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    style_resource_id: u32,
) -> Result<bool> {
    let Some(theme) = theme else {
        return Ok(false);
    };
    let Some(background_attribute_id) =
        layout_attr_resource_id_cached(resources, cache, "background")?
    else {
        return Ok(false);
    };
    let Some(background) = resolve_layout_style_attribute_cached(
        resources,
        theme,
        cache,
        style_resource_id,
        background_attribute_id,
        &mut BTreeSet::new(),
    ) else {
        return Ok(false);
    };

    primitive_is_drawable_reference(resources, theme, cache, &background)
}

pub(crate) fn primitive_is_drawable_reference(
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    primitive: &resource_table::ResourcePrimitive,
) -> Result<bool> {
    primitive_is_drawable_reference_inner(resources, theme, cache, primitive, &mut BTreeSet::new())
}

fn primitive_is_drawable_reference_inner(
    resources: &[u8],
    theme: &LayoutTheme,
    cache: &mut LayoutResolutionCache,
    primitive: &resource_table::ResourcePrimitive,
    visited_attributes: &mut BTreeSet<u32>,
) -> Result<bool> {
    let resource_id = match primitive {
        resource_table::ResourcePrimitive::Reference(resource_id) => *resource_id,
        resource_table::ResourcePrimitive::Attribute(attribute_id) => {
            if !visited_attributes.insert(*attribute_id) {
                return Ok(false);
            }
            let Some(value) =
                resolve_layout_theme_attribute_cached(resources, theme, cache, *attribute_id)
            else {
                return Ok(false);
            };
            return primitive_is_drawable_reference_inner(
                resources,
                theme,
                cache,
                &value,
                visited_attributes,
            );
        }
        _ => return Ok(false),
    };

    resource_id_is_drawable_reference(resources, Some(theme), cache, resource_id)
}

pub(crate) fn resource_id_is_drawable_reference(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    resource_id: u32,
) -> Result<bool> {
    let Some(resource) =
        resolve_layout_resource_value_cached(resources, theme, cache, resource_id)?
    else {
        return Ok(false);
    };

    Ok(matches!(resource.type_name.as_str(), "drawable" | "mipmap"))
}

pub(crate) fn resolve_xml_color_value_cached(
    resources: &[u8],
    theme: Option<&LayoutTheme>,
    cache: &mut LayoutResolutionCache,
    value: &XmlValue,
) -> Result<Option<u32>> {
    match value {
        XmlValue::Color(value) | XmlValue::Hex(value) => Ok(Some(*value)),
        XmlValue::Integer(value) => Ok(Some(*value as u32)),
        XmlValue::Reference(resource_id) => {
            resolve_layout_color_resource_cached(resources, theme, cache, *resource_id).map(Some)
        }
        XmlValue::AttributeReference(attribute_id) => {
            resolve_layout_theme_color_attribute_cached(resources, theme, cache, *attribute_id)
                .map(Some)
        }
        XmlValue::Null
        | XmlValue::String(_)
        | XmlValue::Boolean(_)
        | XmlValue::Float(_)
        | XmlValue::Dimension(_)
        | XmlValue::Fraction(_)
        | XmlValue::Unknown { .. } => Ok(None),
    }
}

pub(crate) fn layout_attr_resource_id_cached(
    resources: &[u8],
    cache: &mut LayoutResolutionCache,
    name: &str,
) -> Result<Option<u32>> {
    if let Some(resource_id) = android_framework_attr_resource_id(name) {
        return Ok(Some(resource_id));
    }

    if let Some(resource_id) = cache.attr_resource_ids.get(name) {
        return Ok(*resource_id);
    }

    let resource_id = resource_table::find_resource_id(resources, "attr", name)?;
    cache.attr_resource_ids.insert(name.to_owned(), resource_id);

    Ok(resource_id)
}

pub(crate) fn android_framework_attr_resource_id(name: &str) -> Option<u32> {
    match name {
        "background" => Some(0x0101_00d4),
        "color" => Some(0x0101_01a5),
        "textColor" => Some(0x0101_0098),
        "tint" => Some(0x0101_0121),
        "backgroundTint" => Some(0x0101_047d),
        "foreground" => Some(0x0101_0109),
        "foregroundTint" => Some(0x0101_046d),
        "windowIsFloating" => Some(0x0101_00b6),
        "windowBackground" => Some(0x0101_0054),
        "backgroundDimEnabled" => Some(0x0101_021f),
        _ => None,
    }
}
