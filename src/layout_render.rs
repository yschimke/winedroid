use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::apk::ApkArchive;
#[cfg(test)]
use crate::binary_xml::XmlEndElement;
use crate::binary_xml::{self, XmlAttribute, XmlEvent, XmlStartElement, XmlValue};
use crate::dex_run::DexViewSnapshot;
use crate::error::{Result, WinedroidError};
use crate::resource_table::{self, ResourceFile};

const MAX_INCLUDE_DEPTH: usize = 16;
const MAX_RENDERED_TREE_LINES_PER_LAYOUT: usize = 24;
const MATCH_PARENT: i32 = -1;
const WRAP_CONTENT: i32 = -2;
const DEFAULT_TEXT_HEIGHT: u32 = 28;
const DEFAULT_IMAGE_HEIGHT: u32 = 120;
const DEFAULT_PROGRESS_HEIGHT: u32 = 32;
const DEFAULT_VIEW_HEIGHT: u32 = 44;
const DEFAULT_TEXT_SIZE: u32 = 16;
const ANDROID_VIEW_VISIBLE: i32 = 0;
const ANDROID_VIEW_INVISIBLE: i32 = 4;
const ANDROID_VIEW_GONE: i32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutRenderSummary {
    pub lines: Vec<String>,
    pub text_elements: Vec<LayoutTextElement>,
    pub view_visibilities: Vec<LayoutViewVisibility>,
    pub scrollable_view_ids: BTreeSet<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutTextElement {
    pub text: String,
    pub kind: LayoutTextKind,
    pub view_id: Option<u32>,
    pub on_click: Option<String>,
    pub drawable_resource_id: Option<u32>,
    pub drawable_padding: u32,
    pub drawable_tint: Option<LayoutDrawableTint>,
    pub drawable: Option<LayoutDrawable>,
    pub text_size: Option<u32>,
    pub text_padding_left: u32,
    pub text_padding_right: u32,
    pub text_color: Option<u32>,
    pub background_color: Option<u32>,
    pub background_corner_radius: u32,
    pub background_border_color: Option<u32>,
    pub background_border_width: u32,
    pub elevation_px: u32,
    pub alignment: LayoutTextAlignment,
    pub editable: bool,
    pub depth: usize,
    pub frame: Option<LayoutFrame>,
    pub clip: Option<LayoutFrame>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutDrawable {
    pub resource_id: u32,
    pub path: String,
    pub data: Vec<u8>,
    pub resources: Vec<u8>,
    pub theme: LayoutTheme,
    pub tint_color: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutDrawableTint {
    Color(u32),
    Resource(u32),
    Attribute(u32),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutTheme {
    pub styles: Vec<LayoutThemeStyle>,
    pub config: resource_table::ResourceResolutionConfig,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutThemeStyle {
    pub resource_id: u32,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LayoutViewVisibility {
    pub view_id: u32,
    pub visibility: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutTextKind {
    Label,
    Button,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayoutTextAlignment {
    Start,
    Center,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutFrame {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

mod content;
mod measure;
mod projection;
mod resource;
mod tree;

pub(crate) use content::*;
pub(crate) use measure::*;
pub(crate) use projection::*;
pub(crate) use resource::*;
pub(crate) use tree::*;

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::binary_xml::{XmlAttribute, XmlValue};

    #[test]
    fn snapshot_layout_ids_are_positive_and_unique() {
        let snapshot = DexViewSnapshot {
            content_layouts: vec![3, 1, -1],
            inflated_layouts: vec![3, 2, 0],
            theme_styles: Vec::new(),
            resource_sdk_version: 35,
            resource_ui_mode: 0,
            resource_display_width: 420,
            resource_display_height: 760,
            resource_screen_round: false,
            runtime_view_roots: Vec::new(),
            runtime_canvas_frames: Vec::new(),
            view_lookup_ids: Vec::new(),
            view_texts: Vec::new(),
            view_images: Vec::new(),
            view_text_cursors: Vec::new(),
            view_visibilities: Vec::new(),
            click_listeners: Vec::new(),
            declared_on_click_handlers: Vec::new(),
            adapter_rows: Vec::new(),
            dynamic_children: Vec::new(),
            activity_overlays: Vec::new(),
            dialog_overlays: Vec::new(),
            action_bar_title: None,
            action_bar_nav_icon: None,
            action_bar_overflow_open: false,
            action_bar_present: false,
            action_bar_home_as_up: false,
            options_menu: Vec::new(),
        };

        assert_eq!(snapshot_layout_ids(&snapshot), vec![1, 2, 3]);
    }

    #[test]
    fn decodes_android_complex_dimensions_to_pixels() {
        assert_eq!(complex_dimension_to_px(0x0000_1001), 16);
        assert_eq!(complex_dimension_to_px(0x0000_0e02), 14);
    }

    #[test]
    fn maps_xml_visibility_to_android_runtime_visibility() {
        let tree = ViewNode {
            name: "TableRow".to_owned(),
            attributes: vec![
                view_attribute("id", XmlValue::Reference(0x7f0901cb)),
                view_attribute("visibility", XmlValue::Integer(2)),
            ],
            children: vec![ViewNode {
                name: "Button".to_owned(),
                attributes: vec![view_attribute("text", XmlValue::String("sin".to_owned()))],
                children: Vec::new(),
            }],
        };
        let mut visibilities = BTreeMap::new();
        let mut elements = Vec::new();

        collect_view_visibilities(&tree, &mut visibilities);
        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(visibilities.get(&0x7f0901cb), Some(&ANDROID_VIEW_GONE));
        assert!(elements.is_empty());
    }

    #[test]
    fn runtime_visibility_overrides_xml_visibility_before_rendering() {
        let mut tree = ViewNode {
            name: "TableRow".to_owned(),
            attributes: vec![
                view_attribute("id", XmlValue::Reference(0x7f0901cb)),
                view_attribute("visibility", XmlValue::Integer(2)),
            ],
            children: vec![ViewNode {
                name: "Button".to_owned(),
                attributes: vec![view_attribute("text", XmlValue::String("sin".to_owned()))],
                children: Vec::new(),
            }],
        };
        let mut runtime_visibilities = BTreeMap::new();
        let mut elements = Vec::new();

        runtime_visibilities.insert(0x7f0901cb, ANDROID_VIEW_VISIBLE);
        apply_runtime_visibility(&mut tree, &runtime_visibilities);
        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: "sin".to_owned(),
                kind: LayoutTextKind::Button,
                view_id: None,
                on_click: None,
                drawable_resource_id: None,
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 100,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn formats_layout_summary_from_start_elements() {
        let resource = ResourceFile {
            resource_id: 0x7f0e0001,
            type_name: "layout".to_owned(),
            key_name: "main".to_owned(),
            path: "res/layout/main.xml".to_owned(),
        };
        let events = vec![
            XmlEvent::Start(XmlStartElement {
                name: "LinearLayout".to_owned(),
                namespace: None,
                attributes: Vec::new(),
            }),
            XmlEvent::Start(XmlStartElement {
                name: "TextView".to_owned(),
                namespace: None,
                attributes: Vec::new(),
            }),
            XmlEvent::End(XmlEndElement {
                name: "TextView".to_owned(),
                namespace: None,
            }),
        ];

        assert_eq!(
            format_layout_summary(&resource, &events),
            "layout 0x7f0e0001 main -> res/layout/main.xml root=LinearLayout elements=LinearLayout > TextView"
        );
    }

    #[test]
    fn builds_view_tree_from_start_and_end_events() {
        let events = vec![
            XmlEvent::Start(XmlStartElement {
                name: "LinearLayout".to_owned(),
                namespace: None,
                attributes: Vec::new(),
            }),
            XmlEvent::Start(XmlStartElement {
                name: "TextView".to_owned(),
                namespace: None,
                attributes: vec![XmlAttribute {
                    name: "text".to_owned(),
                    namespace: None,
                    value: XmlValue::String("Ready".to_owned()),
                }],
            }),
            XmlEvent::End(XmlEndElement {
                name: "TextView".to_owned(),
                namespace: None,
            }),
            XmlEvent::End(XmlEndElement {
                name: "LinearLayout".to_owned(),
                namespace: None,
            }),
        ];

        let tree = build_view_tree(&events, &[])
            .expect("tree should exist")
            .expect("tree should build");

        assert_eq!(tree.name, "LinearLayout");
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].name, "TextView");
        assert_eq!(
            render_view_tree_lines(&tree, 8),
            vec![
                "view LinearLayout".to_owned(),
                "  view TextView text=Ready".to_owned()
            ]
        );
    }

    #[test]
    fn reads_include_layout_reference() {
        let node = ViewNode {
            name: "include".to_owned(),
            attributes: vec![view_attribute("layout", XmlValue::Reference(0x7f0e0001))],
            children: Vec::new(),
        };

        assert_eq!(
            include_layout_reference(&node).expect("include should resolve"),
            0x7f0e0001
        );
    }

    #[test]
    fn collects_visible_text_elements() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![
                ViewNode {
                    name: "TextView".to_owned(),
                    attributes: vec![view_attribute(
                        "text",
                        XmlValue::String("Accept".to_owned()),
                    )],
                    children: Vec::new(),
                },
                ViewNode {
                    name: "TextView".to_owned(),
                    attributes: vec![
                        view_attribute("text", XmlValue::String("Hidden".to_owned())),
                        view_attribute("visibility", XmlValue::Integer(2)),
                    ],
                    children: Vec::new(),
                },
            ],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: "Accept".to_owned(),
                kind: LayoutTextKind::Label,
                view_id: None,
                on_click: None,
                drawable_resource_id: None,
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 28,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn collects_empty_text_attribute_as_text_element() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![ViewNode {
                name: "EditText".to_owned(),
                attributes: vec![
                    view_attribute("id", XmlValue::Reference(0x7f09011b)),
                    view_attribute("text", XmlValue::String(String::new())),
                ],
                children: Vec::new(),
            }],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: String::new(),
                kind: LayoutTextKind::Label,
                view_id: Some(0x7f09011b),
                on_click: None,
                drawable_resource_id: None,
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: true,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 28,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn collects_identified_text_view_without_a_text_attribute() {
        let tree = ViewNode {
            name: "ScrollView".to_owned(),
            attributes: Vec::new(),
            children: vec![ViewNode {
                name: "TextView".to_owned(),
                attributes: vec![view_attribute("id", XmlValue::Reference(0x7f05_0021))],
                children: Vec::new(),
            }],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].view_id, Some(0x7f05_0021));
        assert_eq!(elements[0].text, "");
        assert_eq!(elements[0].kind, LayoutTextKind::Label);
    }

    #[test]
    fn does_not_collect_text_view_without_an_id() {
        let tree = ViewNode {
            name: "TextView".to_owned(),
            attributes: Vec::new(),
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert!(elements.is_empty());
    }

    #[test]
    fn collects_image_button_elements_without_text() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![ViewNode {
                name: "ImageButton".to_owned(),
                attributes: vec![
                    view_attribute("id", XmlValue::Reference(0x7f0900e1)),
                    view_attribute("src", XmlValue::Reference(0x7f0800aa)),
                    view_attribute("layout_width", XmlValue::Integer(WRAP_CONTENT)),
                    view_attribute("layout_height", XmlValue::Integer(WRAP_CONTENT)),
                ],
                children: Vec::new(),
            }],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: String::new(),
                kind: LayoutTextKind::Button,
                view_id: Some(0x7f0900e1),
                on_click: None,
                drawable_resource_id: Some(0x7f0800aa),
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 0,
                    width: DEFAULT_VIEW_HEIGHT,
                    height: DEFAULT_VIEW_HEIGHT,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn collects_image_view_elements_without_text() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![ViewNode {
                name: "ImageView".to_owned(),
                attributes: vec![
                    view_attribute("id", XmlValue::Reference(0x7f090208)),
                    view_attribute("srcCompat", XmlValue::Reference(0x7f0800ab)),
                    view_attribute("layout_width", XmlValue::Integer(64)),
                    view_attribute("layout_height", XmlValue::Integer(48)),
                ],
                children: Vec::new(),
            }],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: String::new(),
                kind: LayoutTextKind::Label,
                view_id: Some(0x7f090208),
                on_click: None,
                drawable_resource_id: Some(0x7f0800ab),
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 48,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn collects_text_view_compound_left_drawable() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![ViewNode {
                name: "TextView".to_owned(),
                attributes: vec![
                    view_attribute("id", XmlValue::Reference(0x7f090015)),
                    view_attribute("text", XmlValue::String("Donate".to_owned())),
                    view_attribute("drawableLeftCompat", XmlValue::Reference(0x7f080094)),
                    view_attribute("drawablePadding", XmlValue::Dimension(0x0000_1401)),
                    view_attribute("color", XmlValue::AttributeReference(0x7f0404b8)),
                    view_attribute("layout_width", XmlValue::Integer(MATCH_PARENT)),
                    view_attribute("layout_height", XmlValue::Integer(WRAP_CONTENT)),
                ],
                children: Vec::new(),
            }],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: "Donate".to_owned(),
                kind: LayoutTextKind::Label,
                view_id: Some(0x7f090015),
                on_click: None,
                drawable_resource_id: Some(0x7f080094),
                drawable_padding: 20,
                drawable_tint: Some(LayoutDrawableTint::Attribute(0x7f0404b8)),
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 28,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn collects_text_color_from_active_theme_attribute() {
        let text_color_attr = 0x7f02_0000;
        let resources = style_resource_table(&[TestStyleResource {
            name: "AppTheme",
            parent: None,
            entries: vec![TestStyleEntry {
                name: text_color_attr,
                value_type: 0x1c,
                data: 0xff12_3456,
            }],
        }]);
        let theme = LayoutTheme {
            styles: vec![LayoutThemeStyle {
                resource_id: 0x7f01_0000,
                force: true,
            }],
            config: resource_table::ResourceResolutionConfig::default(),
        };
        let tree = ViewNode {
            name: "TextView".to_owned(),
            attributes: vec![
                view_attribute("text", XmlValue::String("Theme".to_owned())),
                view_attribute("textColor", XmlValue::AttributeReference(text_color_attr)),
            ],
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 120,
                height: 44,
            },
            Some(&resources),
            Some(&theme),
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(elements[0].text_color, Some(0xff12_3456));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::Theme::getAttribute
    #[test]
    fn theme_color_attribute_chain_uses_aosp_twenty_reference_bound() {
        let mut entries = (0..21)
            .map(|index| TestStyleEntry {
                name: 0x7f04_0000 + index,
                value_type: 0x02,
                data: 0x7f04_0000 + index + 1,
            })
            .collect::<Vec<_>>();
        entries.push(TestStyleEntry {
            name: 0x7f04_0015,
            value_type: 0x1c,
            data: 0xff12_3456,
        });
        let resources = style_resource_table(&[TestStyleResource {
            name: "AppTheme",
            parent: None,
            entries,
        }]);
        let theme = LayoutTheme {
            styles: vec![LayoutThemeStyle {
                resource_id: 0x7f01_0000,
                force: true,
            }],
            config: resource_table::ResourceResolutionConfig::default(),
        };

        let boundary = resolve_layout_theme_color_attribute(&resources, Some(&theme), 0x7f04_0001)
            .expect("twenty theme attribute references should resolve");
        let error = resolve_layout_theme_color_attribute(&resources, Some(&theme), 0x7f04_0000)
            .expect_err("twenty-one theme attribute references should fail");

        assert_eq!(boundary, 0xff12_3456);
        assert!(error.to_string().contains("reference depth exceeded"));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
    #[test]
    fn configured_dimension_resource_aliases_use_aosp_bound_and_explicit_errors() {
        let names = (0..22)
            .map(|index| format!("dimension_{index:02}"))
            .collect::<Vec<_>>();
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let mut default_entries = (0..21)
            .map(|index| (0x01, 0x7f01_0000 + index + 1))
            .collect::<Vec<_>>();
        default_entries.push((0x05, 0x0000_0801));
        let mut configured_entries = default_entries[..21].to_vec();
        configured_entries.push((0x05, 0x0000_1801));
        let resources = configured_test_resource_table(
            "dimen",
            &name_refs,
            &[],
            &default_entries,
            &configured_entries,
        );
        let theme = LayoutTheme {
            styles: Vec::new(),
            config: resource_table::ResourceResolutionConfig::android_with_display(
                35, 0, 1000, 600,
            ),
        };
        let mut cache = LayoutResolutionCache::default();
        let boundary = resolve_layout_dimension_resource_inner(
            &resources,
            Some(&theme),
            &mut cache,
            0x7f01_0001,
        )
        .expect("twenty configured dimension references should resolve");
        let depth_error = resolve_layout_dimension_resource_inner(
            &resources,
            Some(&theme),
            &mut cache,
            0x7f01_0000,
        )
        .expect_err("twenty-one configured dimension references should fail");

        let cycle_entries = [(0x01, 0x7f01_0001), (0x01, 0x7f01_0000)];
        let cycle_resources = configured_test_resource_table(
            "dimen",
            &["cycle_a", "cycle_b"],
            &[],
            &cycle_entries,
            &cycle_entries,
        );
        let cycle_error = resolve_layout_dimension_resource_inner(
            &cycle_resources,
            Some(&theme),
            &mut LayoutResolutionCache::default(),
            0x7f01_0000,
        )
        .expect_err("configured dimension cycle should fail");

        let missing_entries = [(0x01, 0x7f01_0001)];
        let missing_resources = configured_test_resource_table(
            "dimen",
            &["missing"],
            &[],
            &missing_entries,
            &missing_entries,
        );
        let missing_error = resolve_layout_dimension_resource_inner(
            &missing_resources,
            Some(&theme),
            &mut LayoutResolutionCache::default(),
            0x7f01_0000,
        )
        .expect_err("configured missing dimension alias should fail");

        assert_eq!(boundary, 24);
        assert!(depth_error.to_string().contains("exceeded 20 references"));
        assert!(cycle_error.to_string().contains("reference cycle"));
        assert!(missing_error
            .to_string()
            .contains("references missing resource"));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::Theme::getAttribute
    #[test]
    fn dimension_theme_attribute_aliases_use_aosp_bound_and_explicit_errors() {
        let mut entries = (0..21)
            .map(|index| TestStyleEntry {
                name: 0x7f04_0000 + index,
                value_type: 0x02,
                data: 0x7f04_0000 + index + 1,
            })
            .collect::<Vec<_>>();
        entries.extend([
            TestStyleEntry {
                name: 0x7f04_0015,
                value_type: 0x05,
                data: 0x0000_1801,
            },
            TestStyleEntry {
                name: 0x7f04_0100,
                value_type: 0x02,
                data: 0x7f04_0100,
            },
            TestStyleEntry {
                name: 0x7f04_0101,
                value_type: 0x02,
                data: 0x7f04_0102,
            },
        ]);
        let resources = style_resource_table(&[TestStyleResource {
            name: "AppTheme",
            parent: None,
            entries,
        }]);
        let theme = LayoutTheme {
            styles: vec![LayoutThemeStyle {
                resource_id: 0x7f01_0000,
                force: true,
            }],
            config: resource_table::ResourceResolutionConfig::android_with_display(
                35, 0, 1000, 600,
            ),
        };
        let resolve = |attribute_id| {
            resolve_layout_theme_dimension_attribute_cached(
                &resources,
                Some(&theme),
                &mut LayoutResolutionCache::default(),
                attribute_id,
            )
        };

        assert_eq!(
            resolve(0x7f04_0001).expect("twenty dimension attributes should resolve"),
            24
        );
        assert!(resolve(0x7f04_0000)
            .expect_err("twenty-one dimension attributes should fail")
            .to_string()
            .contains("exceeded 20 references"));
        assert!(resolve(0x7f04_0100)
            .expect_err("dimension attribute cycle should fail")
            .to_string()
            .contains("reference cycle"));
        assert!(resolve(0x7f04_0101)
            .expect_err("missing dimension attribute should fail")
            .to_string()
            .contains("references missing attribute"));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
    // cite: frameworks-base ColorStateList.java#inflate, onColorsChanged
    #[test]
    fn configured_color_state_list_aliases_use_aosp_bound_and_explicit_errors() {
        let names = (0..22)
            .map(|index| format!("color_{index:02}"))
            .collect::<Vec<_>>();
        let name_refs = names.iter().map(String::as_str).collect::<Vec<_>>();
        let mut default_entries = (0..21)
            .map(|index| (0x01, 0x7f01_0000 + index + 1))
            .collect::<Vec<_>>();
        default_entries.push((0x1c, 0xffff_0000));
        let mut configured_entries = default_entries[..21].to_vec();
        configured_entries.push((0x1c, 0xff00_ff00));
        let resources = configured_test_resource_table(
            "color",
            &name_refs,
            &[],
            &default_entries,
            &configured_entries,
        );
        let theme = LayoutTheme {
            styles: Vec::new(),
            config: resource_table::ResourceResolutionConfig::android_with_display(
                35, 0, 1000, 600,
            ),
        };
        let (apk_path, mut apk) = empty_test_apk("layout-color-alias");
        let boundary = test_resolve_color_primitive_with_state_lists(
            &mut apk,
            &resources,
            &theme,
            resource_table::ResourcePrimitive::Reference(0x7f01_0001),
        )
        .expect("twenty configured color references should resolve");
        let null_entries = [(0x01, 0x7f01_0001), (0x01, 0)];
        let null_resources = configured_test_resource_table(
            "color",
            &["null_alias", "null_value"],
            &[],
            &null_entries,
            &null_entries,
        );
        let null = test_resolve_color_primitive_with_state_lists(
            &mut apk,
            &null_resources,
            &theme,
            resource_table::ResourcePrimitive::Reference(0x7f01_0000),
        )
        .expect("aliased null color state-list reference should remain absent");
        let depth_error = test_resolve_color_primitive_with_state_lists(
            &mut apk,
            &resources,
            &theme,
            resource_table::ResourcePrimitive::Reference(0x7f01_0000),
        )
        .expect_err("twenty-one configured color references should fail");

        let cycle_entries = [(0x01, 0x7f01_0001), (0x01, 0x7f01_0000)];
        let cycle_resources = configured_test_resource_table(
            "color",
            &["cycle_a", "cycle_b"],
            &[],
            &cycle_entries,
            &cycle_entries,
        );
        let cycle_error = test_resolve_color_primitive_with_state_lists(
            &mut apk,
            &cycle_resources,
            &theme,
            resource_table::ResourcePrimitive::Reference(0x7f01_0000),
        )
        .expect_err("configured color cycle should fail");

        let missing_entries = [(0x01, 0x7f01_0001)];
        let missing_resources = configured_test_resource_table(
            "color",
            &["missing"],
            &[],
            &missing_entries,
            &missing_entries,
        );
        let missing_error = test_resolve_color_primitive_with_state_lists(
            &mut apk,
            &missing_resources,
            &theme,
            resource_table::ResourcePrimitive::Reference(0x7f01_0000),
        )
        .expect_err("configured missing color alias should fail");
        drop(apk);
        std::fs::remove_file(apk_path).expect("test APK should be removed");

        assert_eq!(boundary, Some(0xff00_ff00));
        assert_eq!(null, None);
        assert!(depth_error.to_string().contains("exceeded 20 references"));
        assert!(cycle_error.to_string().contains("reference cycle"));
        assert!(missing_error
            .to_string()
            .contains("references missing resource"));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::Theme::getAttribute
    // cite: frameworks-base ColorStateList.java#inflate, onColorsChanged
    #[test]
    fn color_state_list_attribute_cycle_and_missing_fail_explicitly() {
        let cycle_attribute = 0x7f04_0100;
        let missing_attribute = 0x7f04_0101;
        let resources = style_resource_table(&[TestStyleResource {
            name: "AppTheme",
            parent: None,
            entries: vec![TestStyleEntry {
                name: cycle_attribute,
                value_type: 0x02,
                data: cycle_attribute,
            }],
        }]);
        let theme = LayoutTheme {
            styles: vec![LayoutThemeStyle {
                resource_id: 0x7f01_0000,
                force: true,
            }],
            config: resource_table::ResourceResolutionConfig::android_with_display(
                35, 0, 1000, 600,
            ),
        };
        let (apk_path, mut apk) = empty_test_apk("layout-color-attribute");
        let cycle_error = test_resolve_color_primitive_with_state_lists(
            &mut apk,
            &resources,
            &theme,
            resource_table::ResourcePrimitive::Attribute(cycle_attribute),
        )
        .expect_err("color state-list attribute cycle should fail");
        let missing_error = test_resolve_color_primitive_with_state_lists(
            &mut apk,
            &resources,
            &theme,
            resource_table::ResourcePrimitive::Attribute(missing_attribute),
        )
        .expect_err("missing color state-list attribute should fail");
        drop(apk);
        std::fs::remove_file(apk_path).expect("test APK should be removed");

        assert!(cycle_error.to_string().contains("reference cycle"));
        assert!(missing_error
            .to_string()
            .contains("was not found in active theme"));
    }

    // cite: frameworks-base themes_material.xml#textColorPrimary
    // cite: frameworks-base ResourceTypes.cpp#ResTable::Theme::getAttribute
    #[test]
    fn default_text_color_alias_failure_propagates() {
        let cycle_attribute = 0x7f04_0100;
        let resources = style_resource_table(&[TestStyleResource {
            name: "AppTheme",
            parent: None,
            entries: vec![
                TestStyleEntry {
                    name: 0x0101_0036,
                    value_type: 0x02,
                    data: cycle_attribute,
                },
                TestStyleEntry {
                    name: cycle_attribute,
                    value_type: 0x02,
                    data: cycle_attribute,
                },
            ],
        }]);
        let theme = LayoutTheme {
            styles: vec![LayoutThemeStyle {
                resource_id: 0x7f01_0000,
                force: true,
            }],
            config: resource_table::ResourceResolutionConfig::android_with_display(
                35, 0, 1000, 600,
            ),
        };
        let (apk_path, mut apk) = empty_test_apk("layout-default-text-color");
        let error = resolve_theme_default_text_color(
            &mut apk,
            &resources,
            &theme,
            &mut LayoutResolutionCache::default(),
        )
        .expect_err("default text color alias failure should propagate");
        drop(apk);
        std::fs::remove_file(apk_path).expect("test APK should be removed");

        assert!(error.to_string().contains("reference cycle"));
    }

    // cite: frameworks-base GradientDrawable.java#applyTheme, updateGradientDrawableSolid
    #[test]
    fn gradient_drawable_shape_references_use_active_activity_theme() {
        let shape_color_attr = 0x7f02_0000;
        let resources = style_resource_table(&[TestStyleResource {
            name: "AppTheme",
            parent: None,
            entries: vec![TestStyleEntry {
                name: shape_color_attr,
                value_type: 0x1c,
                data: 0xff12_3456,
            }],
        }]);
        let theme = LayoutTheme {
            styles: vec![LayoutThemeStyle {
                resource_id: 0x7f01_0000,
                force: true,
            }],
            config: resource_table::ResourceResolutionConfig::android_with_display(
                35, 0, 1000, 600,
            ),
        };
        let events = vec![
            XmlEvent::Start(XmlStartElement {
                name: "shape".to_owned(),
                namespace: None,
                attributes: Vec::new(),
            }),
            XmlEvent::Start(XmlStartElement {
                name: "solid".to_owned(),
                namespace: None,
                attributes: vec![XmlAttribute {
                    name: "color".to_owned(),
                    namespace: None,
                    value: XmlValue::AttributeReference(shape_color_attr),
                }],
            }),
        ];

        let style = test_shape_drawable_style_from_events(&resources, &theme, &events)
            .expect("shape drawable should resolve")
            .expect("shape drawable should be recognized");

        assert_eq!(style.solid_color, Some(0xff12_3456));
    }

    #[test]
    fn collects_text_color_from_framework_color_resource() {
        let resources = Vec::new();
        let tree = ViewNode {
            name: "TextView".to_owned(),
            attributes: vec![
                view_attribute("text", XmlValue::String("Framework".to_owned())),
                view_attribute("textColor", XmlValue::Reference(0x0106_0047)),
            ],
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 120,
                height: 44,
            },
            Some(&resources),
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(elements[0].text_color, Some(0xffdc_e2f9));
    }

    #[test]
    fn collects_text_and_background_colors_from_node_style() {
        let resources = style_resource_table(&[
            TestStyleResource {
                name: "UnusedTheme",
                parent: None,
                entries: Vec::new(),
            },
            TestStyleResource {
                name: "ButtonStyle",
                parent: None,
                entries: vec![
                    TestStyleEntry {
                        name: 0x0101_0098,
                        value_type: 0x1c,
                        data: 0xff22_2222,
                    },
                    TestStyleEntry {
                        name: 0x0101_047d,
                        value_type: 0x1c,
                        data: 0xff33_3333,
                    },
                ],
            },
        ]);
        let theme = LayoutTheme {
            styles: Vec::new(),
            config: resource_table::ResourceResolutionConfig::default(),
        };
        let tree = ViewNode {
            name: "Button".to_owned(),
            attributes: vec![
                view_attribute("text", XmlValue::String("Styled".to_owned())),
                view_attribute("style", XmlValue::Reference(0x7f01_0001)),
            ],
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 120,
                height: 44,
            },
            Some(&resources),
            Some(&theme),
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(elements[0].text_color, Some(0xff22_2222));
        assert_eq!(elements[0].background_color, Some(0xff33_3333));
    }

    #[test]
    fn collects_background_only_view_color() {
        let resources = Vec::new();
        let tree = ViewNode {
            name: "View".to_owned(),
            attributes: vec![
                view_attribute("id", XmlValue::Reference(0x7f09_0240)),
                view_attribute("background", XmlValue::Color(0xff12_3456)),
            ],
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 120,
            },
            Some(&resources),
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: String::new(),
                kind: LayoutTextKind::Label,
                view_id: Some(0x7f09_0240),
                on_click: None,
                drawable_resource_id: None,
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: Some(0xff12_3456),
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 0,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 0,
                    width: 200,
                    height: 120,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn style_background_drawable_is_not_treated_as_color() {
        let resources = style_with_drawable_background_resource_table();
        let theme = LayoutTheme {
            styles: Vec::new(),
            config: resource_table::ResourceResolutionConfig::default(),
        };

        let color =
            resolve_style_color_attribute(&resources, Some(&theme), 0x7f01_0000, "background")
                .expect("style background should resolve");

        assert_eq!(color, None);
    }

    #[test]
    fn theme_attribute_drawable_background_is_not_treated_as_color() {
        let background_attr = 0x7f04_0000;
        let resources = style_with_drawable_background_resource_table();
        let theme = LayoutTheme {
            styles: vec![LayoutThemeStyle {
                resource_id: 0x7f01_0001,
                force: true,
            }],
            config: resource_table::ResourceResolutionConfig::default(),
        };
        let mut cache = LayoutResolutionCache::default();
        let primitive = resource_table::ResourcePrimitive::Attribute(background_attr);

        assert!(
            primitive_is_drawable_reference(&resources, &theme, &mut cache, &primitive)
                .expect("attribute drawable reference should resolve")
        );
    }

    #[test]
    fn collects_image_view_background_drawable_without_using_background_tint_as_image_tint() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![ViewNode {
                name: "ImageView".to_owned(),
                attributes: vec![
                    view_attribute("id", XmlValue::Reference(0x7f090010)),
                    view_attribute("background", XmlValue::Reference(0x7f0800a0)),
                    view_attribute("backgroundTint", XmlValue::AttributeReference(0x7f0404b8)),
                    view_attribute("layout_width", XmlValue::Integer(WRAP_CONTENT)),
                    view_attribute("layout_height", XmlValue::Integer(WRAP_CONTENT)),
                ],
                children: Vec::new(),
            }],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: String::new(),
                kind: LayoutTextKind::Label,
                view_id: Some(0x7f090010),
                on_click: None,
                drawable_resource_id: Some(0x7f0800a0),
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 0,
                    width: DEFAULT_VIEW_HEIGHT,
                    height: 100,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::getResource
    #[test]
    fn resource_types_background_drawable_resolution_errors_are_not_missing_images() {
        let node = ViewNode {
            name: "ImageView".to_owned(),
            attributes: vec![view_attribute(
                "background",
                XmlValue::Reference(0x7f01_0000),
            )],
            children: Vec::new(),
        };

        let error = image_drawable_resource_id(
            &node,
            Some(&[0]),
            resource_table::ResourceResolutionConfig::android_with_display(35, 0, 1000, 600),
        )
        .expect_err("malformed selected resource table should propagate");

        assert!(error.to_string().contains("resource table header"));
    }

    #[test]
    fn image_button_background_tint_does_not_tint_image_or_paint_drawable_background() {
        let resources = style_with_drawable_background_resource_table();
        let tree = ViewNode {
            name: "ImageButton".to_owned(),
            attributes: vec![
                view_attribute("id", XmlValue::Reference(0x7f09_0030)),
                view_attribute("src", XmlValue::Reference(0x7f02_0000)),
                view_attribute("background", XmlValue::Reference(0x7f02_0000)),
                view_attribute("backgroundTint", XmlValue::Color(0xff00_0000)),
            ],
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 48,
                height: 56,
            },
            Some(&resources),
            None,
            &mut elements,
        )
        .expect("image button should collect");

        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].drawable_resource_id, Some(0x7f02_0000));
        assert_eq!(elements[0].drawable_tint, None);
        assert_eq!(elements[0].background_color, None);
    }

    #[test]
    fn regular_button_style_background_tint_paints_drawable_background() {
        let resources = style_with_drawable_background_resource_table();
        let theme = LayoutTheme {
            styles: Vec::new(),
            config: resource_table::ResourceResolutionConfig::default(),
        };
        let tree = ViewNode {
            name: "Button".to_owned(),
            attributes: vec![
                view_attribute("text", XmlValue::String("OK".to_owned())),
                view_attribute("style", XmlValue::Reference(0x7f01_0000)),
            ],
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 80,
                height: 44,
            },
            Some(&resources),
            Some(&theme),
            &mut elements,
        )
        .expect("button should collect");

        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].background_color, Some(0xff12_3456));
    }

    #[test]
    fn image_button_style_background_tint_does_not_paint_drawable_background() {
        let resources = style_with_drawable_background_resource_table();
        let theme = LayoutTheme {
            styles: Vec::new(),
            config: resource_table::ResourceResolutionConfig::default(),
        };
        let tree = ViewNode {
            name: "ImageButton".to_owned(),
            attributes: vec![
                view_attribute("id", XmlValue::Reference(0x7f09_0030)),
                view_attribute("src", XmlValue::Reference(0x7f02_0000)),
                view_attribute("style", XmlValue::Reference(0x7f01_0000)),
            ],
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 48,
                height: 56,
            },
            Some(&resources),
            Some(&theme),
            &mut elements,
        )
        .expect("image button should collect");

        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].background_color, None);
    }

    #[test]
    fn regular_button_color_attribute_can_paint_background() {
        let tree = ViewNode {
            name: "Button".to_owned(),
            attributes: vec![
                view_attribute("text", XmlValue::String("OK".to_owned())),
                view_attribute("color", XmlValue::Color(0xff12_3456)),
            ],
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 80,
                height: 44,
            },
            Some(&[]),
            None,
            &mut elements,
        )
        .expect("button should collect");

        assert_eq!(elements.len(), 1);
        assert_eq!(elements[0].background_color, Some(0xff12_3456));
    }

    #[test]
    fn image_button_color_attribute_tints_without_painting_background() {
        let tree = ViewNode {
            name: "ImageButton".to_owned(),
            attributes: vec![
                view_attribute("id", XmlValue::Reference(0x7f09_0030)),
                view_attribute("src", XmlValue::Reference(0x7f02_0000)),
                view_attribute("color", XmlValue::Color(0xff00_0000)),
            ],
            children: Vec::new(),
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 48,
                height: 56,
            },
            Some(&[]),
            None,
            &mut elements,
        )
        .expect("image button should collect");

        assert_eq!(elements.len(), 1);
        assert_eq!(
            elements[0].drawable_tint,
            Some(LayoutDrawableTint::Color(0xff00_0000))
        );
        assert_eq!(elements[0].background_color, None);
    }

    #[test]
    fn collects_image_view_with_id_for_runtime_drawable_state() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![ViewNode {
                name: "ImageView".to_owned(),
                attributes: vec![
                    view_attribute("id", XmlValue::Reference(0x7f090208)),
                    view_attribute("layout_width", XmlValue::Integer(64)),
                    view_attribute("layout_height", XmlValue::Integer(48)),
                ],
                children: Vec::new(),
            }],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: String::new(),
                kind: LayoutTextKind::Label,
                view_id: Some(0x7f090208),
                on_click: None,
                drawable_resource_id: None,
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 0,
                    width: 64,
                    height: 48,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn collects_text_alignment_from_gravity() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![
                ViewNode {
                    name: "TextView".to_owned(),
                    attributes: vec![
                        view_attribute("text", XmlValue::String("Start".to_owned())),
                        view_attribute("gravity", XmlValue::Integer(3)),
                    ],
                    children: Vec::new(),
                },
                ViewNode {
                    name: "TextView".to_owned(),
                    attributes: vec![
                        view_attribute("text", XmlValue::String("Center".to_owned())),
                        view_attribute("gravity", XmlValue::Integer(1)),
                    ],
                    children: Vec::new(),
                },
                ViewNode {
                    name: "TextView".to_owned(),
                    attributes: vec![
                        view_attribute("text", XmlValue::String("End".to_owned())),
                        view_attribute("gravity", XmlValue::Integer(5)),
                    ],
                    children: Vec::new(),
                },
            ],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 300,
                height: 120,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(elements[0].alignment, LayoutTextAlignment::Start);
        assert_eq!(elements[1].alignment, LayoutTextAlignment::Center);
        assert_eq!(elements[2].alignment, LayoutTextAlignment::End);
    }

    // cite: androidx ConstraintLayout.java#onViewAdded, applyConstraintsFromLayoutParams, onLayout
    // cite: androidx Guideline.java#Guideline
    #[test]
    fn positions_constraint_layout_children_against_later_guidelines_and_refs() {
        let menu_id = 0x7f09014f;
        let guideline_id = 0x7f0900ff;
        let tree = ViewNode {
            name: "androidx.constraintlayout.widget.ConstraintLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![
                ViewNode {
                    name: "HorizontalScrollView".to_owned(),
                    attributes: vec![
                        view_attribute("id", XmlValue::Reference(0x7f09011c)),
                        view_attribute("layout_width", XmlValue::Integer(MATCH_PARENT)),
                        view_attribute("layout_height", XmlValue::Integer(0)),
                        view_attribute(
                            "layout_constraintTop_toBottomOf",
                            XmlValue::Reference(menu_id),
                        ),
                        view_attribute(
                            "layout_constraintBottom_toTopOf",
                            XmlValue::Reference(guideline_id),
                        ),
                    ],
                    children: vec![ViewNode {
                        name: "EditText".to_owned(),
                        attributes: vec![
                            view_attribute("id", XmlValue::Reference(0x7f09011b)),
                            view_attribute("text", XmlValue::String(String::new())),
                            view_attribute("layout_width", XmlValue::Integer(WRAP_CONTENT)),
                            view_attribute("layout_height", XmlValue::Integer(MATCH_PARENT)),
                        ],
                        children: Vec::new(),
                    }],
                },
                ViewNode {
                    name: "androidx.constraintlayout.widget.Guideline".to_owned(),
                    attributes: vec![
                        view_attribute("id", XmlValue::Reference(guideline_id)),
                        view_attribute("visibility", XmlValue::Integer(8)),
                        view_attribute("orientation", XmlValue::Integer(0)),
                        view_attribute(
                            "layout_constraintGuide_percent",
                            XmlValue::Float(0.18_f32.to_bits()),
                        ),
                    ],
                    children: Vec::new(),
                },
                ViewNode {
                    name: "ImageButton".to_owned(),
                    attributes: vec![
                        view_attribute("id", XmlValue::Reference(menu_id)),
                        view_attribute("layout_width", XmlValue::Integer(WRAP_CONTENT)),
                        view_attribute("layout_height", XmlValue::Integer(WRAP_CONTENT)),
                        view_attribute("layout_margin", XmlValue::Integer(8)),
                        view_attribute("onClick", XmlValue::String("openAppMenu".to_owned())),
                        view_attribute("layout_constraintEnd_toEndOf", XmlValue::Integer(0)),
                        view_attribute("layout_constraintTop_toTopOf", XmlValue::Integer(0)),
                    ],
                    children: Vec::new(),
                },
            ],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 1000,
                height: 1000,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        // cite: frameworks-base TextView.java#onMeasure
        let edit_text_width = crate::skia_renderer::measured_label_text_width("", 16.0);
        let menu_width = crate::skia_renderer::measured_label_text_width("...", 16.0);

        assert_eq!(
            elements,
            vec![
                LayoutTextElement {
                    text: String::new(),
                    kind: LayoutTextKind::Label,
                    view_id: Some(0x7f09011b),
                    on_click: None,
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: LayoutTextAlignment::Start,
                    editable: true,
                    depth: 2,
                    frame: Some(LayoutFrame {
                        x: 0,
                        y: 52,
                        width: edit_text_width,
                        height: 128,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
                LayoutTextElement {
                    text: "...".to_owned(),
                    kind: LayoutTextKind::Button,
                    view_id: Some(menu_id),
                    on_click: Some("openAppMenu".to_owned()),
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: LayoutTextAlignment::Start,
                    editable: false,
                    depth: 1,
                    frame: Some(LayoutFrame {
                        x: 992 - menu_width,
                        y: 8,
                        width: menu_width,
                        height: DEFAULT_VIEW_HEIGHT,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
            ]
        );
    }

    // cite: androidx ConstraintLayout.java#onViewAdded, applyConstraintsFromLayoutParams, onLayout
    // cite: androidx Guideline.java#Guideline
    #[test]
    fn wrap_content_constraint_layout_retains_gone_guideline_anchor() {
        let guideline_id = 0x7f0900ff;
        let tree = ViewNode {
            name: "androidx.constraintlayout.widget.ConstraintLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![
                ViewNode {
                    name: "androidx.constraintlayout.widget.Guideline".to_owned(),
                    attributes: vec![
                        view_attribute("id", XmlValue::Reference(guideline_id)),
                        view_attribute("visibility", XmlValue::Integer(8)),
                        view_attribute("orientation", XmlValue::Integer(0)),
                        view_attribute("layout_constraintGuide_begin", XmlValue::Integer(100)),
                    ],
                    children: Vec::new(),
                },
                ViewNode {
                    name: "View".to_owned(),
                    attributes: vec![
                        view_attribute("layout_width", XmlValue::Integer(10)),
                        view_attribute("layout_height", XmlValue::Integer(20)),
                        view_attribute(
                            "layout_constraintTop_toBottomOf",
                            XmlValue::Reference(guideline_id),
                        ),
                    ],
                    children: Vec::new(),
                },
            ],
        };

        assert_eq!(constraint_wrap_content_height(&tree, 400, 100), 120);
    }

    #[test]
    fn positions_text_in_vertical_linear_layout() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: vec![
                view_attribute("orientation", XmlValue::Integer(1)),
                view_attribute("paddingTop", XmlValue::Integer(10)),
            ],
            children: vec![ViewNode {
                name: "TextView".to_owned(),
                attributes: vec![
                    view_attribute("text", XmlValue::String("Ready".to_owned())),
                    view_attribute("layout_width", XmlValue::Integer(MATCH_PARENT)),
                    view_attribute("layout_height", XmlValue::Integer(20)),
                ],
                children: Vec::new(),
            }],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 100,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: "Ready".to_owned(),
                kind: LayoutTextKind::Label,
                view_id: None,
                on_click: None,
                drawable_resource_id: None,
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 0,
                    y: 10,
                    width: 200,
                    height: 20,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    #[test]
    fn positions_text_in_table_layout_rows_and_columns() {
        let tree = ViewNode {
            name: "TableLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![
                ViewNode {
                    name: "TableRow".to_owned(),
                    attributes: Vec::new(),
                    children: vec![
                        ViewNode {
                            name: "Button".to_owned(),
                            attributes: vec![
                                view_attribute("id", XmlValue::Reference(0x7f090207)),
                                view_attribute("text", XmlValue::String("7".to_owned())),
                                view_attribute(
                                    "onClick",
                                    XmlValue::String("keyDigitPadMappingToDisplay".to_owned()),
                                ),
                            ],
                            children: Vec::new(),
                        },
                        ViewNode {
                            name: "Button".to_owned(),
                            attributes: vec![
                                view_attribute("id", XmlValue::Reference(0x7f090208)),
                                view_attribute("text", XmlValue::String("8".to_owned())),
                            ],
                            children: Vec::new(),
                        },
                    ],
                },
                ViewNode {
                    name: "TableRow".to_owned(),
                    attributes: Vec::new(),
                    children: vec![ViewNode {
                        name: "Button".to_owned(),
                        attributes: vec![
                            view_attribute("id", XmlValue::Reference(0x7f090204)),
                            view_attribute("text", XmlValue::String("4".to_owned())),
                        ],
                        children: Vec::new(),
                    }],
                },
            ],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 0,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![
                LayoutTextElement {
                    text: "7".to_owned(),
                    kind: LayoutTextKind::Button,
                    view_id: Some(0x7f090207),
                    on_click: Some("keyDigitPadMappingToDisplay".to_owned()),
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: LayoutTextAlignment::Start,
                    editable: false,
                    depth: 2,
                    frame: Some(LayoutFrame {
                        x: 0,
                        y: 0,
                        width: 100,
                        height: DEFAULT_VIEW_HEIGHT,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
                LayoutTextElement {
                    text: "8".to_owned(),
                    kind: LayoutTextKind::Button,
                    view_id: Some(0x7f090208),
                    on_click: None,
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: LayoutTextAlignment::Start,
                    editable: false,
                    depth: 2,
                    frame: Some(LayoutFrame {
                        x: 100,
                        y: 0,
                        width: 100,
                        height: DEFAULT_VIEW_HEIGHT,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
                LayoutTextElement {
                    text: "4".to_owned(),
                    kind: LayoutTextKind::Button,
                    view_id: Some(0x7f090204),
                    on_click: None,
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: LayoutTextAlignment::Start,
                    editable: false,
                    depth: 2,
                    frame: Some(LayoutFrame {
                        x: 0,
                        y: DEFAULT_VIEW_HEIGHT,
                        width: 200,
                        height: DEFAULT_VIEW_HEIGHT,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
            ]
        );
    }

    #[test]
    fn keeps_match_parent_table_rows_to_stable_renderer_height() {
        let tree = ViewNode {
            name: "TableLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![
                ViewNode {
                    name: "TableRow".to_owned(),
                    attributes: vec![view_attribute(
                        "layout_height",
                        XmlValue::Integer(MATCH_PARENT),
                    )],
                    children: vec![ViewNode {
                        name: "Button".to_owned(),
                        attributes: vec![view_attribute("text", XmlValue::String("7".to_owned()))],
                        children: Vec::new(),
                    }],
                },
                ViewNode {
                    name: "TableRow".to_owned(),
                    attributes: vec![view_attribute(
                        "layout_height",
                        XmlValue::Integer(MATCH_PARENT),
                    )],
                    children: vec![ViewNode {
                        name: "Button".to_owned(),
                        attributes: vec![view_attribute("text", XmlValue::String("4".to_owned()))],
                        children: Vec::new(),
                    }],
                },
            ],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 300,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![
                LayoutTextElement {
                    text: "7".to_owned(),
                    kind: LayoutTextKind::Button,
                    view_id: None,
                    on_click: None,
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: LayoutTextAlignment::Start,
                    editable: false,
                    depth: 2,
                    frame: Some(LayoutFrame {
                        x: 0,
                        y: 0,
                        width: 200,
                        height: DEFAULT_VIEW_HEIGHT,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
                LayoutTextElement {
                    text: "4".to_owned(),
                    kind: LayoutTextKind::Button,
                    view_id: None,
                    on_click: None,
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: LayoutTextAlignment::Start,
                    editable: false,
                    depth: 2,
                    frame: Some(LayoutFrame {
                        x: 0,
                        y: DEFAULT_VIEW_HEIGHT,
                        width: 200,
                        height: DEFAULT_VIEW_HEIGHT,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
            ]
        );
    }

    #[test]
    fn distributes_weighted_table_rows_across_available_height() {
        let tree = ViewNode {
            name: "TableLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![
                ViewNode {
                    name: "TableRow".to_owned(),
                    attributes: vec![view_attribute(
                        "layout_weight",
                        XmlValue::Float(1.0_f32.to_bits()),
                    )],
                    children: vec![ViewNode {
                        name: "Button".to_owned(),
                        attributes: vec![view_attribute("text", XmlValue::String("7".to_owned()))],
                        children: Vec::new(),
                    }],
                },
                ViewNode {
                    name: "TableRow".to_owned(),
                    attributes: vec![view_attribute(
                        "layout_weight",
                        XmlValue::Float(1.0_f32.to_bits()),
                    )],
                    children: vec![ViewNode {
                        name: "Button".to_owned(),
                        attributes: vec![view_attribute("text", XmlValue::String("4".to_owned()))],
                        children: Vec::new(),
                    }],
                },
            ],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 300,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![
                LayoutTextElement {
                    text: "7".to_owned(),
                    kind: LayoutTextKind::Button,
                    view_id: None,
                    on_click: None,
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: LayoutTextAlignment::Start,
                    editable: false,
                    depth: 2,
                    frame: Some(LayoutFrame {
                        x: 0,
                        y: 0,
                        width: 200,
                        height: 150,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
                LayoutTextElement {
                    text: "4".to_owned(),
                    kind: LayoutTextKind::Button,
                    view_id: None,
                    on_click: None,
                    drawable_resource_id: None,
                    drawable_padding: 0,
                    drawable_tint: None,
                    drawable: None,
                    text_size: None,
                    text_padding_left: 0,
                    text_padding_right: 0,
                    text_color: None,
                    background_color: None,
                    alignment: LayoutTextAlignment::Start,
                    editable: false,
                    depth: 2,
                    frame: Some(LayoutFrame {
                        x: 0,
                        y: 150,
                        width: 200,
                        height: 150,
                    }),
                    clip: None,
                    background_corner_radius: 0,
                    background_border_color: None,
                    background_border_width: 0,
                    elevation_px: 0,
                },
            ]
        );
    }

    #[test]
    fn positions_relative_text_against_parent_bottom() {
        let tree = ViewNode {
            name: "RelativeLayout".to_owned(),
            attributes: Vec::new(),
            children: vec![ViewNode {
                name: "TextView".to_owned(),
                attributes: vec![
                    view_attribute("text", XmlValue::String("Accept".to_owned())),
                    view_attribute("layout_width", XmlValue::Integer(MATCH_PARENT)),
                    view_attribute("layout_height", XmlValue::Integer(44)),
                    view_attribute("layout_marginLeft", XmlValue::Integer(24)),
                    view_attribute("layout_marginRight", XmlValue::Integer(24)),
                    view_attribute("layout_marginBottom", XmlValue::Integer(16)),
                    view_attribute("layout_alignParentBottom", XmlValue::Boolean(true)),
                ],
                children: Vec::new(),
            }],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 300,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        assert_eq!(
            elements,
            vec![LayoutTextElement {
                text: "Accept".to_owned(),
                kind: LayoutTextKind::Label,
                view_id: None,
                on_click: None,
                drawable_resource_id: None,
                drawable_padding: 0,
                drawable_tint: None,
                drawable: None,
                text_size: None,
                text_padding_left: 0,
                text_padding_right: 0,
                text_color: None,
                background_color: None,
                alignment: LayoutTextAlignment::Start,
                editable: false,
                depth: 1,
                frame: Some(LayoutFrame {
                    x: 24,
                    y: 240,
                    width: 152,
                    height: 44,
                }),
                clip: None,
                background_corner_radius: 0,
                background_border_color: None,
                background_border_width: 0,
                elevation_px: 0,
            }]
        );
    }

    struct TestStyleResource<'a> {
        name: &'a str,
        parent: Option<u32>,
        entries: Vec<TestStyleEntry>,
    }

    struct TestStyleEntry {
        name: u32,
        value_type: u8,
        data: u32,
    }

    pub(crate) fn vector_theme_color_resource_table(attribute_id: u32, color: u32) -> Vec<u8> {
        style_resource_table(&[TestStyleResource {
            name: "AppTheme",
            parent: None,
            entries: vec![TestStyleEntry {
                name: attribute_id,
                value_type: 0x1c,
                data: color,
            }],
        }])
    }

    fn empty_test_apk(label: &str) -> (std::path::PathBuf, ApkArchive) {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock should be after Unix epoch")
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "winedroid-{label}-{}-{unique}.apk",
            std::process::id()
        ));
        let file = std::fs::File::create(&path).expect("test APK should be created");
        zip::ZipWriter::new(file)
            .finish()
            .expect("test APK should finish");
        let apk = ApkArchive::open(&path).expect("test APK should open");
        (path, apk)
    }

    fn configured_test_resource_table(
        type_name: &str,
        key_names: &[&str],
        global_values: &[&str],
        default_entries: &[(u8, u32)],
        configured_entries: &[(u8, u32)],
    ) -> Vec<u8> {
        assert_eq!(key_names.len(), default_entries.len());
        assert_eq!(key_names.len(), configured_entries.len());
        let global_strings = test_resource_string_pool(global_values);
        let type_strings = test_resource_string_pool(&[type_name]);
        let key_strings = test_resource_string_pool(key_names);
        let type_chunks = [
            test_resource_type_chunk(default_entries, Vec::new()),
            test_resource_type_chunk(configured_entries, test_landscape_resource_config()),
        ];
        let type_strings_offset = 288u32;
        let key_strings_offset = type_strings_offset + type_strings.len() as u32;
        let type_chunks_size = type_chunks.iter().map(Vec::len).sum::<usize>();
        let package_size = 288 + type_strings.len() + key_strings.len() + type_chunks_size;
        let mut package = Vec::new();

        push_test_resource_u16(&mut package, 0x0200);
        push_test_resource_u16(&mut package, 288);
        push_test_resource_u32(&mut package, package_size as u32);
        push_test_resource_u32(&mut package, 0x7f);
        package.extend(vec![0; 256]);
        push_test_resource_u32(&mut package, type_strings_offset);
        push_test_resource_u32(&mut package, 0);
        push_test_resource_u32(&mut package, key_strings_offset);
        push_test_resource_u32(&mut package, 0);
        push_test_resource_u32(&mut package, 0);
        package.extend(type_strings);
        package.extend(key_strings);
        for type_chunk in type_chunks {
            package.extend(type_chunk);
        }

        let table_size = 12 + global_strings.len() + package.len();
        let mut table = Vec::new();
        push_test_resource_u16(&mut table, 0x0002);
        push_test_resource_u16(&mut table, 12);
        push_test_resource_u32(&mut table, table_size as u32);
        push_test_resource_u32(&mut table, 1);
        table.extend(global_strings);
        table.extend(package);
        table
    }

    fn test_resource_type_chunk(entries: &[(u8, u32)], config: Vec<u8>) -> Vec<u8> {
        let header_size = 20 + config.len();
        let entries_start = header_size + entries.len() * 4;
        let chunk_size = entries_start + entries.len() * 16;
        let mut chunk = Vec::new();
        push_test_resource_u16(&mut chunk, 0x0201);
        push_test_resource_u16(&mut chunk, header_size as u16);
        push_test_resource_u32(&mut chunk, chunk_size as u32);
        chunk.push(0x01);
        chunk.push(0);
        push_test_resource_u16(&mut chunk, 0);
        push_test_resource_u32(&mut chunk, entries.len() as u32);
        push_test_resource_u32(&mut chunk, entries_start as u32);
        chunk.extend(config);
        for index in 0..entries.len() {
            push_test_resource_u32(&mut chunk, (index * 16) as u32);
        }
        for (key_index, (value_type, value)) in entries.iter().enumerate() {
            push_test_resource_u16(&mut chunk, 8);
            push_test_resource_u16(&mut chunk, 0);
            push_test_resource_u32(&mut chunk, key_index as u32);
            push_test_resource_u16(&mut chunk, 8);
            chunk.push(0);
            chunk.push(*value_type);
            push_test_resource_u32(&mut chunk, *value);
        }
        chunk
    }

    fn test_landscape_resource_config() -> Vec<u8> {
        let mut config = vec![0; 36];
        write_test_resource_u32(&mut config, 0, 36);
        config[12] = 2;
        write_test_resource_u16(&mut config, 32, 1000);
        write_test_resource_u16(&mut config, 34, 600);
        config
    }

    fn test_resource_string_pool(values: &[&str]) -> Vec<u8> {
        let mut string_data = Vec::new();
        let mut offsets = Vec::new();
        for value in values {
            offsets.push(string_data.len() as u32);
            string_data.push(value.len() as u8);
            string_data.push(value.len() as u8);
            string_data.extend(value.as_bytes());
            string_data.push(0);
        }

        let header_size = 28usize;
        let strings_start = header_size + values.len() * 4;
        let chunk_size = strings_start + string_data.len();
        let mut pool = Vec::new();
        push_test_resource_u16(&mut pool, 0x0001);
        push_test_resource_u16(&mut pool, header_size as u16);
        push_test_resource_u32(&mut pool, chunk_size as u32);
        push_test_resource_u32(&mut pool, values.len() as u32);
        push_test_resource_u32(&mut pool, 0);
        push_test_resource_u32(&mut pool, 0x0000_0100);
        push_test_resource_u32(&mut pool, strings_start as u32);
        push_test_resource_u32(&mut pool, 0);
        for offset in offsets {
            push_test_resource_u32(&mut pool, offset);
        }
        pool.extend(string_data);
        pool
    }

    fn push_test_resource_u16(data: &mut Vec<u8>, value: u16) {
        data.extend(value.to_le_bytes());
    }

    fn push_test_resource_u32(data: &mut Vec<u8>, value: u32) {
        data.extend(value.to_le_bytes());
    }

    fn write_test_resource_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn write_test_resource_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn style_resource_table(styles: &[TestStyleResource<'_>]) -> Vec<u8> {
        const RES_STRING_POOL_TYPE: u16 = 0x0001;
        const RES_TABLE_TYPE: u16 = 0x0002;
        const RES_TABLE_PACKAGE_TYPE: u16 = 0x0200;
        const RES_TABLE_TYPE_TYPE: u16 = 0x0201;
        const UTF8_FLAG: u32 = 0x0000_0100;
        const ENTRY_FLAG_COMPLEX: u16 = 0x0001;

        fn push_u16(data: &mut Vec<u8>, value: u16) {
            data.extend(value.to_le_bytes());
        }

        fn push_u32(data: &mut Vec<u8>, value: u32) {
            data.extend(value.to_le_bytes());
        }

        fn push_utf8_string(data: &mut Vec<u8>, value: &str) {
            let length = value.len();
            assert!(length < 0x80);
            data.push(length as u8);
            data.push(length as u8);
            data.extend(value.as_bytes());
            data.push(0);
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

        let global_strings = string_pool(&[]);
        let type_strings = string_pool(&["style"]);
        let key_names = styles.iter().map(|style| style.name).collect::<Vec<_>>();
        let key_strings = string_pool(&key_names);
        let mut entry_offsets = Vec::new();
        let mut entry_data = Vec::new();

        for (style_index, style) in styles.iter().enumerate() {
            entry_offsets.push(entry_data.len() as u32);
            push_u16(&mut entry_data, 16);
            push_u16(&mut entry_data, ENTRY_FLAG_COMPLEX);
            push_u32(&mut entry_data, style_index as u32);
            push_u32(&mut entry_data, style.parent.unwrap_or(0));
            push_u32(&mut entry_data, style.entries.len() as u32);

            for entry in &style.entries {
                push_u32(&mut entry_data, entry.name);
                push_u16(&mut entry_data, 8);
                entry_data.push(0);
                entry_data.push(entry.value_type);
                push_u32(&mut entry_data, entry.data);
            }
        }

        let entries_start = 20 + entry_offsets.len() * 4;
        let type_chunk_size = entries_start + entry_data.len();
        let mut type_chunk = Vec::new();

        push_u16(&mut type_chunk, RES_TABLE_TYPE_TYPE);
        push_u16(&mut type_chunk, 20);
        push_u32(&mut type_chunk, type_chunk_size as u32);
        type_chunk.push(0x01);
        type_chunk.push(0);
        push_u16(&mut type_chunk, 0);
        push_u32(&mut type_chunk, styles.len() as u32);
        push_u32(&mut type_chunk, entries_start as u32);
        for offset in entry_offsets {
            push_u32(&mut type_chunk, offset);
        }
        type_chunk.extend(entry_data);

        let type_strings_offset = 288u32;
        let key_strings_offset = type_strings_offset + type_strings.len() as u32;
        let package_size = 288 + type_strings.len() + key_strings.len() + type_chunk.len();
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
        package.extend(type_chunk);

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

    fn style_with_drawable_background_resource_table() -> Vec<u8> {
        const RES_STRING_POOL_TYPE: u16 = 0x0001;
        const RES_TABLE_TYPE: u16 = 0x0002;
        const RES_TABLE_PACKAGE_TYPE: u16 = 0x0200;
        const RES_TABLE_TYPE_TYPE: u16 = 0x0201;
        const UTF8_FLAG: u32 = 0x0000_0100;
        const ENTRY_FLAG_COMPLEX: u16 = 0x0001;
        const VALUE_TYPE_REFERENCE: u8 = 0x01;
        const VALUE_TYPE_STRING: u8 = 0x03;
        const VALUE_TYPE_COLOR: u8 = 0x1c;

        fn push_u16(data: &mut Vec<u8>, value: u16) {
            data.extend(value.to_le_bytes());
        }

        fn push_u32(data: &mut Vec<u8>, value: u32) {
            data.extend(value.to_le_bytes());
        }

        fn push_utf8_string(data: &mut Vec<u8>, value: &str) {
            let length = value.len();
            assert!(length < 0x80);
            data.push(length as u8);
            data.push(length as u8);
            data.extend(value.as_bytes());
            data.push(0);
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

        fn style_type_chunk() -> Vec<u8> {
            let entries_start = 28usize;
            let first_entry_size = 40usize;
            let second_entry_size = 28usize;
            let chunk_size = entries_start + first_entry_size + second_entry_size;
            let mut chunk = Vec::new();

            push_u16(&mut chunk, RES_TABLE_TYPE_TYPE);
            push_u16(&mut chunk, 20);
            push_u32(&mut chunk, chunk_size as u32);
            chunk.push(0x01);
            chunk.push(0);
            push_u16(&mut chunk, 0);
            push_u32(&mut chunk, 2);
            push_u32(&mut chunk, entries_start as u32);
            push_u32(&mut chunk, 0);
            push_u32(&mut chunk, first_entry_size as u32);

            push_u16(&mut chunk, 16);
            push_u16(&mut chunk, ENTRY_FLAG_COMPLEX);
            push_u32(&mut chunk, 0);
            push_u32(&mut chunk, 0);
            push_u32(&mut chunk, 2);
            push_u32(&mut chunk, 0x0101_00d4);
            push_u16(&mut chunk, 8);
            chunk.push(0);
            chunk.push(VALUE_TYPE_REFERENCE);
            push_u32(&mut chunk, 0x7f02_0000);
            push_u32(&mut chunk, 0x0101_047d);
            push_u16(&mut chunk, 8);
            chunk.push(0);
            chunk.push(VALUE_TYPE_COLOR);
            push_u32(&mut chunk, 0xff12_3456);

            push_u16(&mut chunk, 16);
            push_u16(&mut chunk, ENTRY_FLAG_COMPLEX);
            push_u32(&mut chunk, 1);
            push_u32(&mut chunk, 0);
            push_u32(&mut chunk, 1);
            push_u32(&mut chunk, 0x7f04_0000);
            push_u16(&mut chunk, 8);
            chunk.push(0);
            chunk.push(VALUE_TYPE_REFERENCE);
            push_u32(&mut chunk, 0x7f02_0000);
            chunk
        }

        fn drawable_type_chunk() -> Vec<u8> {
            let entries_start = 24usize;
            let chunk_size = entries_start + 16;
            let mut chunk = Vec::new();

            push_u16(&mut chunk, RES_TABLE_TYPE_TYPE);
            push_u16(&mut chunk, 20);
            push_u32(&mut chunk, chunk_size as u32);
            chunk.push(0x02);
            chunk.push(0);
            push_u16(&mut chunk, 0);
            push_u32(&mut chunk, 1);
            push_u32(&mut chunk, entries_start as u32);
            push_u32(&mut chunk, 0);

            push_u16(&mut chunk, 8);
            push_u16(&mut chunk, 0);
            push_u32(&mut chunk, 2);
            push_u16(&mut chunk, 8);
            chunk.push(0);
            chunk.push(VALUE_TYPE_STRING);
            push_u32(&mut chunk, 0);
            chunk
        }

        let global_strings = string_pool(&["res/q6.xml"]);
        let type_strings = string_pool(&["style", "drawable"]);
        let key_strings = string_pool(&["ButtonStyle", "ThemeStyle", "button_background"]);
        let style_type = style_type_chunk();
        let drawable_type = drawable_type_chunk();
        let type_strings_offset = 288u32;
        let key_strings_offset = type_strings_offset + type_strings.len() as u32;
        let package_size =
            288 + type_strings.len() + key_strings.len() + style_type.len() + drawable_type.len();
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
        package.extend(style_type);
        package.extend(drawable_type);

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

    fn view_attribute(name: &str, typed_value: XmlValue) -> ViewAttribute {
        ViewAttribute {
            name: name.to_owned(),
            value: typed_value.as_text(),
            reference: reference_value(&XmlAttribute {
                name: name.to_owned(),
                namespace: None,
                value: typed_value.clone(),
            }),
            typed_value,
        }
    }

    #[test]
    fn horizontal_row_does_not_starve_sibling_label_height() {
        let tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: vec![view_attribute("orientation", XmlValue::Integer(1))],
            children: vec![
                ViewNode {
                    name: "LinearLayout".to_owned(),
                    attributes: vec![view_attribute("orientation", XmlValue::Integer(0))],
                    children: vec![ViewNode {
                        name: "TextView".to_owned(),
                        attributes: vec![view_attribute(
                            "text",
                            XmlValue::String("USD".to_owned()),
                        )],
                        children: Vec::new(),
                    }],
                },
                ViewNode {
                    name: "TextView".to_owned(),
                    attributes: vec![view_attribute(
                        "text",
                        XmlValue::String("US dollar".to_owned()),
                    )],
                    children: Vec::new(),
                },
            ],
        };
        let mut elements = Vec::new();

        collect_text_elements(
            &tree,
            0,
            LayoutFrame {
                x: 0,
                y: 0,
                width: 200,
                height: 200,
            },
            None,
            None,
            &mut elements,
        )
        .expect("text elements should collect");

        let name = elements
            .iter()
            .find(|element| element.text == "US dollar")
            .expect("second-line label should render");
        let frame = name.frame.expect("label should have a frame");
        assert_eq!(frame.height, 28);
        assert!(frame.y >= 28, "label must sit below the first row");
    }

    #[test]
    fn adapter_rows_render_one_subtree_per_row() {
        let item_tree = ViewNode {
            name: "LinearLayout".to_owned(),
            attributes: vec![view_attribute("orientation", XmlValue::Integer(0))],
            children: vec![
                ViewNode {
                    name: "TextView".to_owned(),
                    attributes: vec![view_attribute("id", XmlValue::Reference(0x10))],
                    children: Vec::new(),
                },
                ViewNode {
                    name: "TextView".to_owned(),
                    attributes: vec![view_attribute("id", XmlValue::Reference(0x11))],
                    children: Vec::new(),
                },
            ],
        };
        let rows = vec![
            AdapterRowRender {
                click_id: 0x6F00_0000,
                texts: BTreeMap::from([(0x10, "USD".to_owned()), (0x11, "US dollar".to_owned())]),
                images: BTreeMap::new(),
                visibilities: BTreeMap::new(),
            },
            AdapterRowRender {
                click_id: 0x6F00_0001,
                texts: BTreeMap::from([(0x10, "GBP".to_owned()), (0x11, "Pound".to_owned())]),
                images: BTreeMap::new(),
                visibilities: BTreeMap::new(),
            },
        ];
        let adapter = AdapterRowsRender {
            item_tree,
            rows,
            image_intrinsic_sizes: BTreeMap::new(),
        };
        let mut elements = Vec::new();

        collect_adapter_rows_elements(
            &adapter,
            LayoutFrame {
                x: 0,
                y: 100,
                width: 300,
                height: 0,
            },
            800,
            None,
            None,
            &mut elements,
        )
        .expect("adapter rows should render");

        let texts: Vec<&str> = elements
            .iter()
            .filter(|element| !element.text.is_empty())
            .map(|element| element.text.as_str())
            .collect();
        assert_eq!(texts, vec!["USD", "US dollar", "GBP", "Pound"]);

        let click_targets: Vec<(u32, u32)> = elements
            .iter()
            .filter(|element| element.text.is_empty())
            .map(|element| {
                (
                    element.view_id.expect("click id"),
                    element.frame.expect("frame").y,
                )
            })
            .collect();
        assert_eq!(click_targets.len(), 2);
        assert_eq!(click_targets[0].0, 0x6F00_0000);
        assert_eq!(click_targets[1].0, 0x6F00_0001);
        assert!(
            click_targets[1].1 > click_targets[0].1,
            "second row must sit below the first"
        );
        assert!(elements
            .iter()
            .filter(|element| !element.text.is_empty())
            .all(|element| element.view_id.is_none()));
    }
}
