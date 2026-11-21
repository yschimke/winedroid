use crate::android_framework;
use crate::apk::ApkArchive;
use crate::binary_xml::{self, XmlEvent};
use crate::resource_table;
use std::collections::{HashMap, HashSet};

use super::*;

const TYPED_VALUE_TYPE_REFERENCE: i32 = 0x01;
const TYPED_VALUE_TYPE_ATTRIBUTE: i32 = 0x02;
const TYPED_VALUE_TYPE_STRING: i32 = 0x03;
const TYPED_VALUE_TYPE_FLOAT: i32 = 0x04;
const TYPED_VALUE_TYPE_DIMENSION: i32 = 0x05;
const TYPED_VALUE_TYPE_FRACTION: i32 = 0x06;
const TYPED_VALUE_TYPE_INT_DEC: i32 = 0x10;
const TYPED_VALUE_TYPE_INT_HEX: i32 = 0x11;
const TYPED_VALUE_TYPE_INT_BOOLEAN: i32 = 0x12;
const TYPED_VALUE_TYPE_INT_COLOR_ARGB8: i32 = 0x1c;
const ANDROID_RESOURCE_NAMESPACE: &str = "http://schemas.android.com/apk/res/android";
const CONFIGURATION_TOUCHSCREEN_NOTOUCH: i32 = 1;
const CONFIGURATION_ORIENTATION_PORTRAIT: i32 = 1;
const CONFIGURATION_SCREENLAYOUT_ROUND_MASK: i32 = 0x300;
const CONFIGURATION_SCREENLAYOUT_ROUND_YES: i32 = 0x200;
const CONFIGURATION_ORIENTATION_LANDSCAPE: i32 = 2;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::dex_run) enum ResolvedColorResource {
    Integer(u32),
    Complex,
}

impl DexProgram {
    pub(in crate::dex_run) fn configuration_init_default(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &configuration_font_scale_field(),
            DexValue::Float(BOOTSTRAP_FONT_SCALE),
        );
        self.write_stored_instance_field(
            &invocation.arguments[0],
            &configuration_ui_mode_field(),
            DexValue::Int(self.config.android_configuration_ui_mode()),
        );

        Some(())
    }

    pub(in crate::dex_run) fn configuration_copy_from(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        let receiver_key = instance_field_owner_key(&invocation.arguments[0])?;
        let source_key = instance_field_owner_key(&invocation.arguments[1])?;
        let updates = self.configuration_field_updates(&source_key);
        let mut copied_font_scale = false;
        let mut copied_ui_mode = false;

        {
            let mut runtime = self.runtime.borrow_mut();
            for (field_signature, value) in updates {
                if configuration_field_name(&field_signature) == Some("fontScale") {
                    copied_font_scale = true;
                }
                if configuration_field_name(&field_signature) == Some("uiMode") {
                    copied_ui_mode = true;
                }

                runtime.insert_field((receiver_key.clone(), field_signature), value);
            }
        }

        if !copied_font_scale {
            self.write_stored_instance_field(
                &invocation.arguments[0],
                &configuration_font_scale_field(),
                DexValue::Float(BOOTSTRAP_FONT_SCALE),
            );
        }
        if !copied_ui_mode {
            self.write_stored_instance_field(
                &invocation.arguments[0],
                &configuration_ui_mode_field(),
                DexValue::Int(self.config.android_configuration_ui_mode()),
            );
        }

        Some(())
    }

    pub(in crate::dex_run) fn configuration_is_screen_round(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let receiver = invocation.arguments.first()?;
        let screen_layout = match self
            .read_stored_instance_field(receiver, &configuration_int_field("screenLayout"))
        {
            Some(DexValue::Int(value)) => value,
            _ => 0,
        };

        // cite: frameworks-base Configuration.java#isScreenRound
        Some(i32::from(
            screen_layout & CONFIGURATION_SCREENLAYOUT_ROUND_MASK
                == CONFIGURATION_SCREENLAYOUT_ROUND_YES,
        ))
    }

    pub(in crate::dex_run) fn write_bootstrap_configuration_fields(&self) {
        let configuration = DexValue::Reference(std::rc::Rc::from(
            "Landroid/content/res/Configuration;.bootstrap",
        ));

        self.write_stored_instance_field(
            &configuration,
            &configuration_font_scale_field(),
            DexValue::Float(BOOTSTRAP_FONT_SCALE),
        );
        self.write_stored_instance_field(
            &configuration,
            &configuration_ui_mode_field(),
            DexValue::Int(self.config.android_configuration_ui_mode()),
        );
        self.write_display_configuration_fields(&configuration);
    }

    fn write_display_configuration_fields(&self, configuration: &DexValue) {
        let display_size = self.display_size();
        let width = display_size.width_i32();
        let height = display_size.height_i32();
        let orientation = if width > height {
            CONFIGURATION_ORIENTATION_LANDSCAPE
        } else {
            CONFIGURATION_ORIENTATION_PORTRAIT
        };
        let screen_layout = resource_table::screen_layout_for_display(
            display_size.width() as u16,
            display_size.height() as u16,
        ) | resource_table::configuration_screen_layout_round_bits(
            self.config.display_shape().is_round(),
        );

        // cite: frameworks-base ResourcesManager.java#applyDisplayMetricsToConfiguration
        for (name, value) in [
            ("touchscreen", CONFIGURATION_TOUCHSCREEN_NOTOUCH),
            ("densityDpi", BOOTSTRAP_DISPLAY_DENSITY_DPI),
            ("screenWidthDp", width),
            ("screenHeightDp", height),
            ("smallestScreenWidthDp", width.min(height)),
            ("compatScreenWidthDp", width),
            ("compatScreenHeightDp", height),
            ("compatSmallestScreenWidthDp", width.min(height)),
            ("orientation", orientation),
            ("screenLayout", screen_layout),
        ] {
            self.write_stored_instance_field(
                configuration,
                &configuration_int_field(name),
                DexValue::Int(value),
            );
        }
    }

    pub(in crate::dex_run) fn configuration_update_from(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        let receiver_key = instance_field_owner_key(&invocation.arguments[0])?;
        let delta_key = instance_field_owner_key(&invocation.arguments[1])?;
        let updates = self.configuration_field_updates(&delta_key);
        let mut changed = 0;
        let mut runtime = self.runtime.borrow_mut();

        for (field_signature, value) in updates {
            let field_key = (receiver_key.clone(), field_signature.clone());
            if runtime.field(&field_key.0, &field_key.1) == Some(&value) {
                continue;
            }

            changed |= configuration_change_mask(&field_signature);
            runtime.insert_field(field_key, value);
        }

        Some(changed)
    }

    fn configuration_field_updates(&self, delta_key: &str) -> Vec<(String, DexValue)> {
        self.runtime
            .borrow()
            .iter_fields()
            .filter(|((owner_key, field_signature), _value)| {
                owner_key.matches_key(delta_key)
                    && configuration_field_name(field_signature).is_some()
            })
            .map(|((_owner_key, field_signature), value)| {
                (field_signature.to_owned(), value.clone())
            })
            .collect()
    }

    pub(in crate::dex_run) fn resources_get_layout(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        let DexValue::Int(resource_id) = invocation.arguments[1] else {
            return None;
        };

        let parser = self.fresh_xml_resource_parser_reference(resource_id);
        let parser_key = map_identity_key(&parser);
        let parser_state = self.load_xml_parser_state(resource_id).ok()?;
        self.runtime
            .borrow_mut()
            .xml_parsers
            .insert(parser_key, parser_state);

        Some(parser)
    }

    pub(in crate::dex_run) fn fresh_xml_resource_parser_reference(
        &self,
        resource_id: i32,
    ) -> DexValue {
        let mut runtime = self.runtime.borrow_mut();
        let parser_id = runtime.next_object_id;
        runtime.next_object_id += 1;

        DexValue::Reference(std::rc::Rc::from(format!(
            "Landroid/content/res/XmlResourceParser;:layout/{resource_id}:parser/{parser_id}"
        )))
    }

    pub(in crate::dex_run) fn resources_get_dimension(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<f32>> {
        let Some(resource_id) = resources_int_resource_id(invocation) else {
            return Ok(None);
        };

        if resource_id <= 0 {
            return Err(invalid_resource_id_error("dimension", resource_id));
        }

        let value = self
            .resolve_dimension_resource_px(resource_id as u32)?
            .ok_or_else(|| resource_type_error("dimension", resource_id as u32))?;

        Ok(Some(value))
    }

    pub(in crate::dex_run) fn resources_get_dimension_pixel_size(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<i32>> {
        let Some(value) = self.resources_get_dimension(invocation)? else {
            return Ok(None);
        };

        Ok(Some(android_dimension_float_to_pixel_size(value)))
    }

    pub(in crate::dex_run) fn resources_get_dimension_pixel_offset(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<i32>> {
        let Some(value) = self.resources_get_dimension(invocation)? else {
            return Ok(None);
        };

        // cite: frameworks-base Resources.java#getDimensionPixelOffset
        // cite: frameworks-base TypedValue.java#complexToDimensionPixelOffset
        Ok(Some(value as i32))
    }

    pub(in crate::dex_run) fn resources_get_boolean(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<i32>> {
        let Some(resource_id) = resources_int_resource_id(invocation) else {
            return Ok(None);
        };

        if resource_id <= 0 {
            return Err(invalid_resource_id_error("boolean", resource_id));
        }

        let value = self
            .resolve_boolean_resource(resource_id as u32)?
            .ok_or_else(|| resource_type_error("boolean", resource_id as u32))?;

        Ok(Some(i32::from(value)))
    }

    pub(in crate::dex_run) fn resources_get_color(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<DexExecutionResult>> {
        let Some(resource_id) = resources_int_resource_id(invocation) else {
            return Ok(None);
        };

        if resource_id <= 0 {
            return Err(invalid_resource_id_error("color", resource_id));
        }

        let value = self
            .resolve_color_resource(resource_id as u32)?
            .ok_or_else(|| resource_type_error("color", resource_id as u32))?;

        match value {
            ResolvedColorResource::Integer(value) => {
                Ok(Some(DexExecutionResult::Int(value as i32)))
            }
            ResolvedColorResource::Complex => Ok(None),
        }
    }

    pub(in crate::dex_run) fn resources_get_value(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<()>> {
        if invocation.arguments.len() != 4 {
            return Ok(None);
        }

        match &invocation.arguments[0] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return Ok(None),
        }

        let DexValue::Int(resource_id) = invocation.arguments[1] else {
            return Ok(None);
        };
        if resource_id <= 0 {
            return Err(invalid_resource_id_error("typed value", resource_id));
        }

        match &invocation.arguments[2] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return Ok(None),
        }

        let DexValue::Int(resolve_references) = invocation.arguments[3] else {
            return Ok(None);
        };

        let resource = self
            .resolve_typed_value_resource_value(resource_id as u32, resolve_references != 0)?
            .ok_or_else(|| resource_type_error("typed value", resource_id as u32))?;
        Ok(self.write_typed_value(
            &invocation.arguments[2],
            resource.resource_id as i32,
            resource.value,
        ))
    }

    #[cfg(test)]
    fn resolve_typed_value_resource(
        &self,
        resource_id: u32,
        resolve_references: bool,
    ) -> Result<Option<resource_table::ResourcePrimitive>> {
        let resource = self.resolve_typed_value_resource_value(resource_id, resolve_references)?;
        Ok(resource.map(|resource| resource.value))
    }

    fn resolve_typed_value_resource_value(
        &self,
        resource_id: u32,
        resolve_references: bool,
    ) -> Result<Option<ResourceValue>> {
        if resource_id == 0 {
            return Ok(Some(ResourceValue {
                resource_id,
                type_name: String::new(),
                key_name: String::new(),
                value: resource_table::ResourcePrimitive::Reference(0),
            }));
        }

        if let Some(value) = android_framework_typed_value_resource(resource_id) {
            return Ok(Some(ResourceValue {
                resource_id,
                type_name: String::new(),
                key_name: String::new(),
                value,
            }));
        }

        let Some(resource) = self.resolve_resource_value(resource_id)? else {
            return Ok(None);
        };
        // cite: frameworks-base AssetManager.java#getResourceValue
        // cite: frameworks-base AssetManager2.cpp#AssetManager2::GetResource
        if matches!(
            &resource.value,
            resource_table::ResourcePrimitive::Array(_) | resource_table::ResourcePrimitive::Bag(_)
        ) {
            return Ok(None);
        }

        if !resolve_references {
            return Ok(Some(resource));
        }

        self.resolve_resource_value_references(
            resource_id,
            Some(android_framework_typed_value_resource),
        )
    }

    fn write_typed_value(
        &self,
        out_value: &DexValue,
        resource_id: i32,
        value: resource_table::ResourcePrimitive,
    ) -> Option<()> {
        let typed_value = typed_value_from_resource_primitive(value)?;

        self.write_runtime_typed_value(out_value, resource_id, typed_value)
    }

    fn typed_array_value_from_resource_reference(
        &self,
        resource_id: u32,
    ) -> Result<Option<(u32, RuntimeTypedValue)>> {
        if resource_id == 0 {
            return Ok(None);
        }

        let resource = self.resolve_typed_array_resource_reference(resource_id)?;
        if resource.value == resource_table::ResourcePrimitive::Reference(0) {
            return Ok(None);
        }
        let resource_id = resource.resource_id;
        let value = match resource.value {
            resource_table::ResourcePrimitive::Array(_)
            | resource_table::ResourcePrimitive::Bag(_) => {
                resource_table::ResourcePrimitive::Reference(resource_id)
            }
            value => value,
        };

        Ok(typed_value_from_resource_primitive(value).map(|value| (resource_id, value)))
    }

    fn typed_value_from_resource_reference(
        &self,
        resource_id: u32,
    ) -> Result<Option<(u32, RuntimeTypedValue)>> {
        if resource_id == 0 {
            return Ok(typed_value_from_resource_primitive(
                resource_table::ResourcePrimitive::Reference(0),
            )
            .map(|value| (0, value)));
        }

        let resource = self.resolve_typed_array_resource_reference(resource_id)?;
        let resource_id = resource.resource_id;

        Ok(typed_value_from_resource_primitive(resource.value).map(|value| (resource_id, value)))
    }

    fn write_runtime_typed_value(
        &self,
        out_value: &DexValue,
        resource_id: i32,
        typed_value: RuntimeTypedValue,
    ) -> Option<()> {
        self.write_stored_instance_field(
            out_value,
            &typed_value_int_field("type"),
            DexValue::Int(typed_value.value_type),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            out_value,
            &typed_value_int_field("data"),
            DexValue::Int(typed_value.data),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            out_value,
            &typed_value_char_sequence_field("string"),
            typed_value.string.unwrap_or(DexValue::Null),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            out_value,
            &typed_value_int_field("assetCookie"),
            DexValue::Int(0),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            out_value,
            &typed_value_int_field("resourceId"),
            DexValue::Int(resource_id),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            out_value,
            &typed_value_int_field("changingConfigurations"),
            DexValue::Int(0),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            out_value,
            &typed_value_int_field("density"),
            DexValue::Int(0),
        )
        .then_some(())?;

        Some(())
    }

    pub(in crate::dex_run) fn typed_array_get_text(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<DexValue>> {
        let Some(index) = typed_array_index(invocation) else {
            return Ok(None);
        };
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            let value = self.typed_array_resource_primitive_text(value)?;
            return Ok(Some(
                value
                    .map(|value| DexValue::String(value.into()))
                    .unwrap_or(DexValue::Null),
            ));
        }

        let value = match self.typed_array_xml_attribute_value(&invocation.arguments[0], index) {
            Some(binary_xml::XmlValue::Reference(0) | binary_xml::XmlValue::Null) => None,
            Some(binary_xml::XmlValue::Reference(resource_id)) => {
                let resource = self.resolve_typed_array_resource_reference(resource_id)?;
                self.typed_array_resource_primitive_text(resource.value)?
            }
            Some(value) => self.xml_value_text(&value)?,
            None => None,
        };

        Ok(Some(
            value
                .map(|value| DexValue::String(value.into()))
                .unwrap_or(DexValue::Null),
        ))
    }

    pub(in crate::dex_run) fn typed_array_get_text_array(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<DexValue>> {
        let Some(index) = typed_array_index(invocation) else {
            return Ok(None);
        };
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            return self.typed_array_text_array_from_primitive(value);
        }
        let Some(value) = self.typed_array_xml_attribute_value(&invocation.arguments[0], index)
        else {
            return Ok(Some(DexValue::Null));
        };
        match value {
            binary_xml::XmlValue::Null | binary_xml::XmlValue::Reference(0) => {
                Ok(Some(DexValue::Null))
            }
            binary_xml::XmlValue::Reference(resource_id) => self
                .typed_array_text_array_from_primitive(
                    resource_table::ResourcePrimitive::Reference(resource_id),
                ),
            _ => self.typed_array_text_array_from_resource_id(0),
        }
    }

    pub(in crate::dex_run) fn typed_array_get_drawable(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        let index = typed_array_index(invocation)?;
        let value = self
            .typed_array_xml_attribute_value(&invocation.arguments[0], index)
            .and_then(|value| self.xml_value_drawable(&value));

        Some(value.unwrap_or(DexValue::Null))
    }

    pub(in crate::dex_run) fn typed_array_get_resource_id(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let (index, default_value) = typed_array_index_and_default_int(invocation)?;
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            if value == resource_table::ResourcePrimitive::Reference(0) {
                return Some(default_value);
            }
            return Some(resource_primitive_resource_id(&value).unwrap_or(default_value));
        }

        let value = self
            .typed_array_xml_attribute_value(&invocation.arguments[0], index)
            .and_then(|value| xml_value_resource_id(&value));

        Some(value.unwrap_or(default_value))
    }

    pub(in crate::dex_run) fn typed_array_get_float(
        &self,
        invocation: &DexInvocation,
    ) -> Option<f32> {
        let (index, default_value) = typed_array_index_and_default_float(invocation)?;
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            return Some(resource_primitive_float(&value).unwrap_or(default_value));
        }

        let value = self
            .typed_array_xml_attribute_value(&invocation.arguments[0], index)
            .and_then(|value| xml_value_float(&value));

        Some(value.unwrap_or(default_value))
    }

    pub(in crate::dex_run) fn typed_array_get_dimension(
        &self,
        invocation: &DexInvocation,
    ) -> Option<f32> {
        let (index, default_value) = typed_array_index_and_default_float(invocation)?;
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            return Some(resource_primitive_float(&value).unwrap_or(default_value));
        }

        let value = self
            .typed_array_xml_attribute_value(&invocation.arguments[0], index)
            .and_then(|value| xml_value_float(&value));

        Some(value.unwrap_or(default_value))
    }

    // cite: frameworks-base TypedArray.java#getDimensionPixelOffset
    pub(in crate::dex_run) fn typed_array_get_dimension_pixel_offset(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let (index, default_value) = typed_array_index_and_default_int(invocation)?;
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            // cite: frameworks-base AttributeResolution.cpp#ResolveAttrs, ApplyStyle
            if value == resource_table::ResourcePrimitive::Reference(0) {
                return Some(default_value);
            }
            let resource_table::ResourcePrimitive::Dimension(value) = value else {
                return None;
            };

            return Some(android_complex_dimension_to_pixel_offset(value));
        }

        let Some(value) = self.typed_array_xml_attribute_value(&invocation.arguments[0], index)
        else {
            return Some(default_value);
        };
        match value {
            binary_xml::XmlValue::Null | binary_xml::XmlValue::Reference(0) => Some(default_value),
            binary_xml::XmlValue::Dimension(value) => {
                Some(android_complex_dimension_to_pixel_offset(value))
            }
            _ => None,
        }
    }

    // cite: frameworks-base TypedArray.java#getDimensionPixelSize
    pub(in crate::dex_run) fn typed_array_get_dimension_pixel_size(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let (index, default_value) = typed_array_index_and_default_int(invocation)?;
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            // cite: frameworks-base AttributeResolution.cpp#ResolveAttrs, ApplyStyle
            if value == resource_table::ResourcePrimitive::Reference(0) {
                return Some(default_value);
            }
            let resource_table::ResourcePrimitive::Dimension(value) = value else {
                return None;
            };

            return Some(android_complex_dimension_to_pixel_size(value));
        }

        let Some(value) = self.typed_array_xml_attribute_value(&invocation.arguments[0], index)
        else {
            return Some(default_value);
        };
        match value {
            binary_xml::XmlValue::Null | binary_xml::XmlValue::Reference(0) => Some(default_value),
            binary_xml::XmlValue::Dimension(value) => {
                Some(android_complex_dimension_to_pixel_size(value))
            }
            _ => None,
        }
    }

    // cite: frameworks-base TypedArray.java#getLayoutDimension
    pub(in crate::dex_run) fn typed_array_get_layout_dimension(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let (index, default_value) = typed_array_index_and_default_int(invocation)?;
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            return Some(match value {
                resource_table::ResourcePrimitive::Dimension(value) => {
                    android_complex_dimension_to_pixel_size(value)
                }
                value => resource_primitive_raw_int(&value).unwrap_or(default_value),
            });
        }

        let Some(value) = self.typed_array_xml_attribute_value(&invocation.arguments[0], index)
        else {
            return Some(default_value);
        };

        Some(match value {
            binary_xml::XmlValue::Dimension(value) => {
                android_complex_dimension_to_pixel_size(value)
            }
            value => xml_value_raw_int(&value).unwrap_or(default_value),
        })
    }

    pub(in crate::dex_run) fn typed_array_get_value(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<i32>> {
        let Some((index, out_value)) = typed_array_get_value_arguments(invocation) else {
            return Ok(None);
        };
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            // cite: frameworks-base AttributeResolution.cpp#ResolveAttrs, ApplyStyle
            // cite: frameworks-base TypedArray.java#getValue, getValueAt
            if value == resource_table::ResourcePrimitive::Reference(0) {
                return Ok(Some(0));
            }
            let (resource_id, value) = match value {
                resource_table::ResourcePrimitive::Reference(resource_id) => {
                    let resource = self.resolve_typed_array_resource_reference(resource_id)?;
                    if resource.value == resource_table::ResourcePrimitive::Reference(0) {
                        return Ok(Some(0));
                    }
                    let resource_id = resource.resource_id;
                    let value = match resource.value {
                        resource_table::ResourcePrimitive::Array(_)
                        | resource_table::ResourcePrimitive::Bag(_) => {
                            resource_table::ResourcePrimitive::Reference(resource_id)
                        }
                        value => value,
                    };
                    (resource_id as i32, value)
                }
                value => (0, value),
            };
            let Some(()) = self.write_typed_value(out_value, resource_id, value) else {
                return Ok(None);
            };
            return Ok(Some(1));
        }

        let Some(value) = self.typed_array_xml_attribute_value(&invocation.arguments[0], index)
        else {
            return Ok(Some(0));
        };
        if matches!(
            &value,
            binary_xml::XmlValue::Null | binary_xml::XmlValue::Reference(0)
        ) {
            return Ok(Some(0));
        }

        let Some((resource_id, typed_value)) = self.typed_value_from_xml_attribute_value(value)?
        else {
            return Ok(Some(0));
        };
        let Some(()) = self.write_runtime_typed_value(out_value, resource_id, typed_value) else {
            return Ok(None);
        };

        Ok(Some(1))
    }

    pub(in crate::dex_run) fn typed_array_has_value(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let index = typed_array_index(invocation)?;
        let typed_array_key = map_identity_key(&invocation.arguments[0]);
        if let Some(typed_array) = self.runtime.borrow().typed_arrays.get(&typed_array_key) {
            // cite: frameworks-base AttributeResolution.cpp#ResolveAttrs, ApplyStyle
            // cite: frameworks-base TypedArray.java#hasValue
            return Some(i32::from(matches!(
                typed_array.values.get(&index),
                Some(value) if value != &resource_table::ResourcePrimitive::Reference(0)
            )));
        }

        let value = self.typed_array_xml_attribute_value(&invocation.arguments[0], index);
        Some(i32::from(!matches!(
            value,
            None | Some(binary_xml::XmlValue::Null) | Some(binary_xml::XmlValue::Reference(0))
        )))
    }

    pub(in crate::dex_run) fn typed_array_get_boolean(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let (index, default_value) = typed_array_index_and_default_int(invocation)?;
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            return Some(resource_primitive_bool(&value).unwrap_or(default_value != 0) as i32);
        }

        typed_array_default_boolean(invocation)
    }

    // cite: frameworks-base TypedArray.java#getInt
    // cite: frameworks-base XmlUtils.java#convertValueToInt
    pub(in crate::dex_run) fn typed_array_get_int(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let (index, default_value) = typed_array_index_and_default_int(invocation)?;
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            return Some(resource_primitive_int(&value).unwrap_or(default_value));
        }

        let Some(value) = self.typed_array_xml_attribute_value(&invocation.arguments[0], index)
        else {
            return Some(default_value);
        };

        match &value {
            binary_xml::XmlValue::Null | binary_xml::XmlValue::Reference(0) => Some(default_value),
            binary_xml::XmlValue::String(value) if value.is_empty() => Some(default_value),
            _ => xml_value_int(&value),
        }
    }

    // cite: frameworks-base TypedArray.java#getColor
    pub(in crate::dex_run) fn typed_array_get_color(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<DexExecutionResult>> {
        let Some((index, default_value)) = typed_array_index_and_default_int(invocation) else {
            return Ok(None);
        };
        if let Some(value) = self.typed_array_resolved_value(&invocation.arguments[0], index) {
            return self.typed_array_color_from_primitive(value, index, default_value);
        }

        let Some(value) = self.typed_array_xml_attribute_value(&invocation.arguments[0], index)
        else {
            return Ok(Some(DexExecutionResult::Int(default_value)));
        };

        let result = match value {
            binary_xml::XmlValue::Color(value) | binary_xml::XmlValue::Hex(value) => {
                DexExecutionResult::Int(value as i32)
            }
            binary_xml::XmlValue::Integer(value) => DexExecutionResult::Int(value),
            binary_xml::XmlValue::Boolean(value) => {
                DexExecutionResult::Int(if value { -1 } else { 0 })
            }
            binary_xml::XmlValue::Unknown {
                data_type, data, ..
            } if (0x10..=0x1f).contains(&data_type) => DexExecutionResult::Int(data as i32),
            binary_xml::XmlValue::Reference(0) | binary_xml::XmlValue::Null => {
                DexExecutionResult::Int(default_value)
            }
            binary_xml::XmlValue::Reference(resource_id) => {
                return self.typed_array_color_from_primitive(
                    resource_table::ResourcePrimitive::Reference(resource_id),
                    index,
                    default_value,
                );
            }
            binary_xml::XmlValue::String(_) => return Ok(None),
            binary_xml::XmlValue::AttributeReference(_) => {
                self.typed_array_unsupported_color(index, TYPED_VALUE_TYPE_ATTRIBUTE)
            }
            binary_xml::XmlValue::Float(_) => {
                self.typed_array_unsupported_color(index, TYPED_VALUE_TYPE_FLOAT)
            }
            binary_xml::XmlValue::Dimension(_) => {
                self.typed_array_unsupported_color(index, TYPED_VALUE_TYPE_DIMENSION)
            }
            binary_xml::XmlValue::Fraction(_) => {
                self.typed_array_unsupported_color(index, TYPED_VALUE_TYPE_FRACTION)
            }
            binary_xml::XmlValue::Unknown { data_type, .. } => {
                self.typed_array_unsupported_color(index, i32::from(data_type))
            }
        };
        Ok(Some(result))
    }

    fn typed_array_color_from_primitive(
        &self,
        value: resource_table::ResourcePrimitive,
        index: i32,
        default_value: i32,
    ) -> Result<Option<DexExecutionResult>> {
        let value = match value {
            resource_table::ResourcePrimitive::Reference(0) => {
                return Ok(Some(DexExecutionResult::Int(default_value)));
            }
            resource_table::ResourcePrimitive::Reference(resource_id) => {
                self.resolve_typed_array_resource_reference(resource_id)?
                    .value
            }
            value => value,
        };
        if let Some(data) = resource_primitive_integer_family_data(&value) {
            return Ok(Some(DexExecutionResult::Int(data as i32)));
        }
        let result = match value {
            resource_table::ResourcePrimitive::Reference(0) => {
                DexExecutionResult::Int(default_value)
            }
            resource_table::ResourcePrimitive::Integer(_)
            | resource_table::ResourcePrimitive::Hex(_)
            | resource_table::ResourcePrimitive::Boolean(_)
            | resource_table::ResourcePrimitive::Color(_) => unreachable!(),
            resource_table::ResourcePrimitive::String(_) => return Ok(None),
            value @ (resource_table::ResourcePrimitive::Reference(_)
            | resource_table::ResourcePrimitive::Attribute(_)
            | resource_table::ResourcePrimitive::Float(_)
            | resource_table::ResourcePrimitive::Dimension(_)
            | resource_table::ResourcePrimitive::Fraction(_)
            | resource_table::ResourcePrimitive::Array(_)
            | resource_table::ResourcePrimitive::Bag(_)
            | resource_table::ResourcePrimitive::Unknown { .. }) => {
                let value_type = typed_value_from_resource_primitive(value)
                    .map_or(TYPED_VALUE_TYPE_REFERENCE, |value| value.value_type);
                self.typed_array_unsupported_color(index, value_type)
            }
        };

        Ok(Some(result))
    }

    fn typed_array_unsupported_color(&self, index: i32, value_type: i32) -> DexExecutionResult {
        // cite: frameworks-base TypedArray.java#getColor
        DexExecutionResult::Throw(self.allocate_generated_exception(DexGeneratedException {
            class_descriptor: "Ljava/lang/UnsupportedOperationException;".to_owned(),
            detail_message: Some(format!(
                "Can't convert value at index {index} to color: type=0x{value_type:x}"
            )),
        }))
    }

    pub(in crate::dex_run) fn typed_array_get_color_state_list(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<DexExecutionResult>> {
        let Some(index) = typed_array_index(invocation) else {
            return Ok(None);
        };
        let value = match self.typed_array_resolved_value(&invocation.arguments[0], index) {
            Some(value) => value,
            None => match self.typed_array_xml_attribute_value(&invocation.arguments[0], index) {
                None
                | Some(binary_xml::XmlValue::Null)
                | Some(binary_xml::XmlValue::Reference(0)) => {
                    return Ok(Some(DexExecutionResult::Value(DexValue::Null)));
                }
                Some(binary_xml::XmlValue::Reference(resource_id)) => {
                    resource_table::ResourcePrimitive::Reference(resource_id)
                }
                Some(binary_xml::XmlValue::Color(value)) => {
                    resource_table::ResourcePrimitive::Color(value)
                }
                Some(binary_xml::XmlValue::Hex(value)) => {
                    resource_table::ResourcePrimitive::Hex(value)
                }
                Some(binary_xml::XmlValue::Integer(value)) => {
                    resource_table::ResourcePrimitive::Integer(value)
                }
                Some(binary_xml::XmlValue::Boolean(value)) => {
                    resource_table::ResourcePrimitive::Boolean(value)
                }
                Some(binary_xml::XmlValue::String(value)) => {
                    resource_table::ResourcePrimitive::String(value)
                }
                Some(binary_xml::XmlValue::AttributeReference(value)) => {
                    resource_table::ResourcePrimitive::Attribute(value)
                }
                Some(binary_xml::XmlValue::Float(value)) => {
                    resource_table::ResourcePrimitive::Float(value)
                }
                Some(binary_xml::XmlValue::Dimension(value)) => {
                    resource_table::ResourcePrimitive::Dimension(value)
                }
                Some(binary_xml::XmlValue::Fraction(value)) => {
                    resource_table::ResourcePrimitive::Fraction(value)
                }
                Some(binary_xml::XmlValue::Unknown {
                    data_type, data, ..
                }) => resource_table::ResourcePrimitive::Unknown { data_type, data },
            },
        };
        let value = match value {
            resource_table::ResourcePrimitive::Reference(0) => {
                return Ok(Some(DexExecutionResult::Value(DexValue::Null)));
            }
            resource_table::ResourcePrimitive::Reference(resource_id) => {
                self.resolve_typed_array_resource_reference(resource_id)?
                    .value
            }
            value => value,
        };
        let color = match value {
            resource_table::ResourcePrimitive::Color(value) => value as i32,
            resource_table::ResourcePrimitive::String(_) => return Ok(None),
            resource_table::ResourcePrimitive::Reference(0) => {
                return Ok(Some(DexExecutionResult::Value(DexValue::Null)));
            }
            value => {
                let value_type = typed_value_from_resource_primitive(value)
                    .map_or(TYPED_VALUE_TYPE_REFERENCE, |value| value.value_type);
                // cite: frameworks-base TypedArray.java#getColorStateList
                // cite: frameworks-base ResourcesImpl.java#loadColorStateList, loadComplexColorFromName
                return Ok(Some(DexExecutionResult::Throw(
                    self.allocate_generated_exception(DexGeneratedException {
                        class_descriptor: "Ljava/lang/UnsupportedOperationException;".to_owned(),
                        detail_message: Some(format!(
                            "Can't convert value at index {index} to ColorStateList: type=0x{value_type:x}"
                        )),
                    }),
                )));
            }
        };
        let invocation = DexInvocation {
            arguments: smallvec::smallvec![DexValue::Int(color)],
            ..invocation.clone()
        };

        Ok(self
            .color_state_list_value_of(&invocation)
            .map(DexExecutionResult::Value))
    }

    fn typed_array_resolved_value(
        &self,
        typed_array: &DexValue,
        index: i32,
    ) -> Option<resource_table::ResourcePrimitive> {
        let typed_array_key = map_identity_key(typed_array);
        self.runtime
            .borrow()
            .typed_arrays
            .get(&typed_array_key)?
            .values
            .get(&index)
            .cloned()
    }

    fn typed_value_from_xml_attribute_value(
        &self,
        value: binary_xml::XmlValue,
    ) -> Result<Option<(i32, RuntimeTypedValue)>> {
        let resource_id = xml_typed_value_resource_id(&value);

        if let binary_xml::XmlValue::Reference(reference_id) = value {
            let Some((resource_id, typed_value)) =
                self.typed_array_value_from_resource_reference(reference_id)?
            else {
                return Ok(None);
            };
            return Ok(Some((resource_id as i32, typed_value)));
        }

        Ok(typed_value_from_xml_value(value).map(|value| (resource_id, value)))
    }

    pub(in crate::dex_run) fn typed_array_xml_attribute_value(
        &self,
        typed_array: &DexValue,
        index: i32,
    ) -> Option<binary_xml::XmlValue> {
        let index = usize::try_from(index).ok()?;
        let typed_array_key = map_identity_key(typed_array);
        let runtime = self.runtime.borrow();
        let parser = runtime
            .xml_parsers
            .iter()
            .find(|(key, _)| typed_array_reference_contains_key(&typed_array_key, key))
            .map(|(_, parser)| parser)?;
        let XmlParserPosition::Event(event_index) = parser.position else {
            return None;
        };
        let XmlEvent::Start(element) = parser.events.get(event_index)? else {
            return None;
        };
        let styleable_entries = runtime
            .array_entries
            .iter()
            .find(|(key, _)| typed_array_reference_contains_key(&typed_array_key, key))
            .map(|(_, values)| values)?;
        let styleable_entry = styleable_entries.get(index)?;
        let attribute_name = self.styleable_attribute_name(styleable_entry)?;
        let attribute_resource_id = Self::styleable_attribute_resource_id(styleable_entry);
        // cite: frameworks-base AttributeResolution.cpp#XmlAttributeFinder
        // cite: frameworks-base ResourceTypes.cpp#getAttributeNameResID
        let attribute = match attribute_resource_id {
            Some(resource_id) if resource_id >> 24 == 0x01 => {
                element.attributes.iter().find(|attribute| {
                    attribute.name == attribute_name
                        && attribute.namespace.as_deref() == Some(ANDROID_RESOURCE_NAMESPACE)
                })
            }
            Some(_) => element.attributes.iter().find(|attribute| {
                attribute.name == attribute_name
                    && attribute.namespace.as_deref() != Some(ANDROID_RESOURCE_NAMESPACE)
            }),
            None => element.attribute(&attribute_name),
        }?;

        Some(attribute.value.clone())
    }

    pub(in crate::dex_run) fn styleable_attribute_name(&self, value: &DexValue) -> Option<String> {
        match value {
            DexValue::Int(resource_id) if *resource_id > 0 => {
                self.resource_key_name(*resource_id as u32)
            }
            DexValue::String(name) => Some(name.clone().to_string()),
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::Object(_)
            | DexValue::Reference(_) => None,
        }
    }

    fn styleable_attribute_resource_id(value: &DexValue) -> Option<u32> {
        match value {
            DexValue::Int(resource_id) if *resource_id > 0 => Some(*resource_id as u32),
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_)
            | DexValue::Object(_)
            | DexValue::Reference(_) => None,
        }
    }

    pub(in crate::dex_run) fn resource_key_name(&self, resource_id: u32) -> Option<String> {
        if let Some(name) = android_framework_attr_name(resource_id) {
            return Some(name.to_owned());
        }

        self.resolve_resource_value(resource_id)
            .ok()
            .flatten()
            .map(|resource| resource.key_name)
    }

    pub(in crate::dex_run) fn resource_type_and_key_name(
        &self,
        resource_id: u32,
    ) -> Option<(String, String)> {
        self.resolve_resource_value(resource_id)
            .ok()
            .flatten()
            .map(|resource| (resource.type_name, resource.key_name))
    }

    pub(in crate::dex_run) fn xml_value_text(
        &self,
        value: &binary_xml::XmlValue,
    ) -> Result<Option<String>> {
        match value {
            binary_xml::XmlValue::String(value) => Ok(Some(value.clone())),
            binary_xml::XmlValue::Reference(resource_id) => {
                self.resolve_string_resource_text(*resource_id)
            }
            binary_xml::XmlValue::Boolean(value) => Ok(Some(value.to_string())),
            binary_xml::XmlValue::Integer(value) => Ok(Some(value.to_string())),
            binary_xml::XmlValue::Hex(value) => Ok(Some(format!("0x{value:08x}"))),
            binary_xml::XmlValue::Null
            | binary_xml::XmlValue::AttributeReference(_)
            | binary_xml::XmlValue::Float(_)
            | binary_xml::XmlValue::Dimension(_)
            | binary_xml::XmlValue::Fraction(_)
            | binary_xml::XmlValue::Color(_)
            | binary_xml::XmlValue::Unknown { .. } => Ok(None),
        }
    }

    pub(in crate::dex_run) fn xml_value_drawable(
        &self,
        value: &binary_xml::XmlValue,
    ) -> Option<DexValue> {
        match value {
            binary_xml::XmlValue::Reference(resource_id) if *resource_id != 0 => {
                Some(self.drawable_resource_value(*resource_id))
            }
            binary_xml::XmlValue::Null
            | binary_xml::XmlValue::Integer(0)
            | binary_xml::XmlValue::Hex(0) => Some(DexValue::Null),
            binary_xml::XmlValue::String(_)
            | binary_xml::XmlValue::Boolean(_)
            | binary_xml::XmlValue::Integer(_)
            | binary_xml::XmlValue::Hex(_)
            | binary_xml::XmlValue::Reference(_)
            | binary_xml::XmlValue::AttributeReference(_)
            | binary_xml::XmlValue::Float(_)
            | binary_xml::XmlValue::Dimension(_)
            | binary_xml::XmlValue::Fraction(_)
            | binary_xml::XmlValue::Color(_)
            | binary_xml::XmlValue::Unknown { .. } => None,
        }
    }

    pub(in crate::dex_run) fn resolve_string_resource_text(
        &self,
        resource_id: u32,
    ) -> Result<Option<String>> {
        let Some(value) = self.resolve_resource_primitive_references(resource_id, None)? else {
            return Ok(None);
        };

        match value {
            resource_table::ResourcePrimitive::Reference(next_resource_id) => {
                Ok(Some(format!("@{next_resource_id}")))
            }
            resource_table::ResourcePrimitive::Array(_)
            | resource_table::ResourcePrimitive::Bag(_) => Ok(None),
            value => Ok(resource_primitive_coerce_to_string(&value)),
        }
    }

    pub(in crate::dex_run) fn resolve_text_array_resource(
        &self,
        resource_id: u32,
    ) -> Result<Option<Vec<Option<String>>>> {
        let Some(value) = self.resolve_array_resource_value_references(resource_id)? else {
            return Ok(None);
        };
        let value = value.value;

        match value {
            resource_table::ResourcePrimitive::Array(values) => {
                let mut strings = Vec::with_capacity(values.len());
                for value in values {
                    strings.push(self.resource_primitive_text(value)?);
                }
                Ok(Some(strings))
            }
            resource_table::ResourcePrimitive::Reference(0) => Ok(None),
            resource_table::ResourcePrimitive::Reference(next_resource_id) => Err(
                unresolved_terminal_reference_error(resource_id, next_resource_id),
            ),
            resource_table::ResourcePrimitive::String(_)
            | resource_table::ResourcePrimitive::Attribute(_)
            | resource_table::ResourcePrimitive::Float(_)
            | resource_table::ResourcePrimitive::Dimension(_)
            | resource_table::ResourcePrimitive::Fraction(_)
            | resource_table::ResourcePrimitive::Integer(_)
            | resource_table::ResourcePrimitive::Hex(_)
            | resource_table::ResourcePrimitive::Boolean(_)
            | resource_table::ResourcePrimitive::Color(_)
            | resource_table::ResourcePrimitive::Bag(_)
            | resource_table::ResourcePrimitive::Unknown { .. } => Ok(None),
        }
    }

    fn resource_primitive_text(
        &self,
        value: resource_table::ResourcePrimitive,
    ) -> Result<Option<String>> {
        match value {
            resource_table::ResourcePrimitive::Reference(0) => Ok(None),
            resource_table::ResourcePrimitive::Reference(resource_id) => {
                let Some(resource) = self.resolve_resource_value_references(resource_id, None)?
                else {
                    return Ok(None);
                };
                match resource.value {
                    resource_table::ResourcePrimitive::String(value) => Ok(Some(value)),
                    _ => Ok(None),
                }
            }
            resource_table::ResourcePrimitive::String(value) => Ok(Some(value)),
            resource_table::ResourcePrimitive::Attribute(_)
            | resource_table::ResourcePrimitive::Float(_)
            | resource_table::ResourcePrimitive::Dimension(_)
            | resource_table::ResourcePrimitive::Fraction(_)
            | resource_table::ResourcePrimitive::Integer(_)
            | resource_table::ResourcePrimitive::Hex(_)
            | resource_table::ResourcePrimitive::Boolean(_)
            | resource_table::ResourcePrimitive::Color(_)
            | resource_table::ResourcePrimitive::Array(_)
            | resource_table::ResourcePrimitive::Bag(_)
            | resource_table::ResourcePrimitive::Unknown { .. } => Ok(None),
        }
    }

    fn typed_array_resource_primitive_text(
        &self,
        value: resource_table::ResourcePrimitive,
    ) -> Result<Option<String>> {
        let value = match value {
            resource_table::ResourcePrimitive::Reference(0) => return Ok(None),
            resource_table::ResourcePrimitive::Reference(resource_id) => {
                let resource = self.resolve_typed_array_resource_reference(resource_id)?;
                if resource.value == resource_table::ResourcePrimitive::Reference(0) {
                    return Ok(None);
                }
                resource.value
            }
            value => value,
        };

        Ok(resource_primitive_coerce_to_string(&value))
    }

    fn typed_array_text_array_from_primitive(
        &self,
        value: resource_table::ResourcePrimitive,
    ) -> Result<Option<DexValue>> {
        let resource_id = match value {
            resource_table::ResourcePrimitive::Reference(0) => {
                return Ok(Some(DexValue::Null));
            }
            resource_table::ResourcePrimitive::Reference(resource_id) => {
                let resource = self.resolve_typed_array_resource_reference(resource_id)?;
                if resource.value == resource_table::ResourcePrimitive::Reference(0) {
                    return Ok(Some(DexValue::Null));
                }
                resource.resource_id
            }
            _ => 0,
        };

        self.typed_array_text_array_from_resource_id(resource_id)
    }

    fn typed_array_text_array_from_resource_id(
        &self,
        resource_id: u32,
    ) -> Result<Option<DexValue>> {
        // cite: frameworks-base TypedArray.java#getTextArray
        let values = self
            .resolve_text_array_resource(resource_id)?
            .ok_or_else(|| resource_type_error("text array", resource_id))?;
        let values = values
            .into_iter()
            .map(|value| {
                value
                    .map(|value| DexValue::String(value.into()))
                    .unwrap_or(DexValue::Null)
            })
            .collect();

        Ok(Some(self.allocate_filled_array(
            "[Ljava/lang/CharSequence;".to_owned(),
            values,
        )))
    }

    pub(in crate::dex_run) fn resolve_boolean_resource(
        &self,
        resource_id: u32,
    ) -> Result<Option<bool>> {
        let Some(value) = self.resolve_resource_primitive_references(resource_id, None)? else {
            return Ok(None);
        };

        match value {
            resource_table::ResourcePrimitive::Boolean(value) => Ok(Some(value)),
            resource_table::ResourcePrimitive::Integer(value) => Ok(Some(value != 0)),
            resource_table::ResourcePrimitive::Hex(value) => Ok(Some(value != 0)),
            resource_table::ResourcePrimitive::Color(value) => Ok(Some(value != 0)),
            resource_table::ResourcePrimitive::Unknown { data_type, data }
                if (0x10..=0x1f).contains(&data_type) =>
            {
                Ok(Some(data != 0))
            }
            resource_table::ResourcePrimitive::Reference(0) => Ok(None),
            resource_table::ResourcePrimitive::Reference(next_resource_id) => Err(
                unresolved_terminal_reference_error(resource_id, next_resource_id),
            ),
            resource_table::ResourcePrimitive::String(_)
            | resource_table::ResourcePrimitive::Attribute(_)
            | resource_table::ResourcePrimitive::Float(_)
            | resource_table::ResourcePrimitive::Dimension(_)
            | resource_table::ResourcePrimitive::Fraction(_)
            | resource_table::ResourcePrimitive::Array(_)
            | resource_table::ResourcePrimitive::Bag(_)
            | resource_table::ResourcePrimitive::Unknown { .. } => Ok(None),
        }
    }

    pub(in crate::dex_run) fn resolve_color_resource(
        &self,
        resource_id: u32,
    ) -> Result<Option<ResolvedColorResource>> {
        let Some(value) = self.resolve_resource_primitive_references(
            resource_id,
            Some(android_framework_color_resource),
        )?
        else {
            return Ok(None);
        };

        match value {
            resource_table::ResourcePrimitive::Color(value) => {
                Ok(Some(ResolvedColorResource::Integer(value)))
            }
            resource_table::ResourcePrimitive::Hex(value) => {
                Ok(Some(ResolvedColorResource::Integer(value)))
            }
            resource_table::ResourcePrimitive::Integer(value) => {
                Ok(Some(ResolvedColorResource::Integer(value as u32)))
            }
            resource_table::ResourcePrimitive::Boolean(value) => {
                Ok(Some(ResolvedColorResource::Integer(if value {
                    u32::MAX
                } else {
                    0
                })))
            }
            resource_table::ResourcePrimitive::Unknown { data_type, data }
                if (0x10..=0x1f).contains(&data_type) =>
            {
                Ok(Some(ResolvedColorResource::Integer(data)))
            }
            resource_table::ResourcePrimitive::String(_) => {
                Ok(Some(ResolvedColorResource::Complex))
            }
            resource_table::ResourcePrimitive::Reference(0) => Ok(None),
            resource_table::ResourcePrimitive::Reference(next_resource_id) => Err(
                unresolved_terminal_reference_error(resource_id, next_resource_id),
            ),
            resource_table::ResourcePrimitive::Attribute(_)
            | resource_table::ResourcePrimitive::Float(_)
            | resource_table::ResourcePrimitive::Dimension(_)
            | resource_table::ResourcePrimitive::Fraction(_)
            | resource_table::ResourcePrimitive::Array(_)
            | resource_table::ResourcePrimitive::Bag(_)
            | resource_table::ResourcePrimitive::Unknown { .. } => Ok(None),
        }
    }

    pub(in crate::dex_run) fn resolve_dimension_resource_px(
        &self,
        resource_id: u32,
    ) -> Result<Option<f32>> {
        let Some(value) = self.resolve_resource_primitive_references(resource_id, None)? else {
            return Ok(None);
        };

        match value {
            resource_table::ResourcePrimitive::Dimension(value) => {
                Ok(Some(android_complex_dimension_to_px_float(value)))
            }
            resource_table::ResourcePrimitive::Reference(0) => Ok(None),
            resource_table::ResourcePrimitive::Reference(next_resource_id) => Err(
                unresolved_terminal_reference_error(resource_id, next_resource_id),
            ),
            resource_table::ResourcePrimitive::String(_)
            | resource_table::ResourcePrimitive::Attribute(_)
            | resource_table::ResourcePrimitive::Float(_)
            | resource_table::ResourcePrimitive::Fraction(_)
            | resource_table::ResourcePrimitive::Integer(_)
            | resource_table::ResourcePrimitive::Hex(_)
            | resource_table::ResourcePrimitive::Boolean(_)
            | resource_table::ResourcePrimitive::Color(_)
            | resource_table::ResourcePrimitive::Array(_)
            | resource_table::ResourcePrimitive::Bag(_)
            | resource_table::ResourcePrimitive::Unknown { .. } => Ok(None),
        }
    }

    fn resolve_resource_primitive_references(
        &self,
        resource_id: u32,
        framework_resource: Option<fn(u32) -> Option<resource_table::ResourcePrimitive>>,
    ) -> Result<Option<resource_table::ResourcePrimitive>> {
        let resource = self.resolve_resource_value_references(resource_id, framework_resource)?;

        Ok(resource.map(|resource| resource.value))
    }

    fn resolve_resource_value_references(
        &self,
        resource_id: u32,
        framework_resource: Option<fn(u32) -> Option<resource_table::ResourcePrimitive>>,
    ) -> Result<Option<ResourceValue>> {
        let initial =
            if let Some(value) = framework_resource.and_then(|resolve| resolve(resource_id)) {
                Some(ResourceValue {
                    resource_id,
                    type_name: String::new(),
                    key_name: String::new(),
                    value,
                })
            } else {
                self.resolve_resource_value(resource_id)?
            };
        let Some(initial) = initial else {
            return Ok(None);
        };
        // cite: frameworks-base AssetManager.java#getResourceValue
        // cite: frameworks-base AssetManager2.cpp#AssetManager2::GetResource
        if matches!(
            &initial.value,
            resource_table::ResourcePrimitive::Array(_) | resource_table::ResourcePrimitive::Bag(_)
        ) {
            return Ok(None);
        }

        resource_table::resolve_resource_value_references_for_resources_with(
            resource_id,
            |current_id| {
                if let Some(value) = framework_resource.and_then(|resolve| resolve(current_id)) {
                    return Ok(Some(ResourceValue {
                        resource_id: current_id,
                        type_name: String::new(),
                        key_name: String::new(),
                        value,
                    }));
                }

                let Some(mut resource) = self.resolve_resource_value(current_id)? else {
                    return Ok(None);
                };
                if matches!(
                    &resource.value,
                    resource_table::ResourcePrimitive::Array(_)
                        | resource_table::ResourcePrimitive::Bag(_)
                ) {
                    resource.value = resource_table::ResourcePrimitive::Reference(current_id);
                }
                Ok(Some(resource))
            },
        )
    }

    fn resolve_array_resource_value_references(
        &self,
        resource_id: u32,
    ) -> Result<Option<ResourceValue>> {
        resource_table::resolve_resource_value_references_for_resources_with(
            resource_id,
            |current_id| self.resolve_resource_value(current_id),
        )
    }

    fn resolve_typed_array_resource_reference(&self, resource_id: u32) -> Result<ResourceValue> {
        // cite: frameworks-base AttributeResolution.cpp#ResolveAttrs, ApplyStyle, RetrieveAttributes
        // cite: frameworks-base AssetManager2.cpp#AssetManager2::ResolveReference
        resource_table::resolve_resource_value_references_preserving_unresolved_with(
            resource_id,
            |current_id| {
                if let Some(value) = android_framework_typed_value_resource(current_id) {
                    return Ok(Some(ResourceValue {
                        resource_id: current_id,
                        type_name: String::new(),
                        key_name: String::new(),
                        value,
                    }));
                }

                let Some(mut resource) = self.resolve_resource_value(current_id)? else {
                    return Ok(None);
                };
                // cite: frameworks-base AssetManager2.cpp#AssetManager2::GetResource, AssetManager2::ResolveReference
                if matches!(
                    &resource.value,
                    resource_table::ResourcePrimitive::Array(_)
                        | resource_table::ResourcePrimitive::Bag(_)
                ) {
                    resource.value = resource_table::ResourcePrimitive::Reference(current_id);
                }
                Ok(Some(resource))
            },
        )?
        .ok_or_else(|| resource_type_error("typed value", resource_id))
    }

    // cite: frameworks-base Resources.java#NotFoundException
    pub(in crate::dex_run) fn resource_not_found_exception(
        &self,
        detail_message: impl ToString,
    ) -> DexValue {
        self.allocate_generated_exception(DexGeneratedException {
            class_descriptor: "Landroid/content/res/Resources$NotFoundException;".to_owned(),
            detail_message: Some(detail_message.to_string()),
        })
    }

    pub(in crate::dex_run) fn resource_not_found_for_request(
        &self,
        invocation: &DexInvocation,
        resource_kind: &str,
    ) -> DexValue {
        let detail_message = match invocation.arguments.get(1) {
            Some(DexValue::Int(resource_id)) => {
                format!("{resource_kind} resource ID #0x{resource_id:08x} was not found")
            }
            Some(value) => {
                format!("{resource_kind} resource request has a non-integer resource ID: {value}")
            }
            None => format!("{resource_kind} resource request has no resource ID"),
        };
        self.resource_not_found_exception(detail_message)
    }

    pub(in crate::dex_run) fn load_xml_parser_state(
        &self,
        resource_id: i32,
    ) -> Result<XmlParserState> {
        if resource_id <= 0 {
            return Err(WinedroidError::new(format!(
                "XML parser resource ID must be positive\nresource id: {resource_id}"
            )));
        }

        let apk_path = self.apk_path.as_ref().ok_or_else(|| {
            WinedroidError::new("XML parser requested but APK path is unavailable")
        })?;
        let resource_id = resource_id as u32;
        let resource = self.resolve_resource_file(resource_id)?.ok_or_else(|| {
            WinedroidError::new(format!(
                "XML parser resource 0x{resource_id:08x} was not found in resources.arsc"
            ))
        })?;
        let mut apk = ApkArchive::open(apk_path)?;
        let data = apk.read_required_file(&resource.path)?;
        let events = binary_xml::parse_binary_xml(&data).map_err(|error| {
            WinedroidError::with_source(
                format!(
                    "failed to parse XML resource 0x{resource_id:08x} at `{}`",
                    resource.path
                ),
                error,
            )
        })?;

        Ok(XmlParserState {
            resource_id: resource_id as i32,
            events,
            position: XmlParserPosition::StartDocument,
            closed: false,
        })
    }

    pub(in crate::dex_run) fn open_raw_resource(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 2 {
            return None;
        }
        let DexValue::Int(resource_id) = invocation.arguments[1] else {
            return None;
        };

        let bytes = self.read_raw_resource_bytes(resource_id).ok()?;
        let id = {
            let mut runtime = self.runtime.borrow_mut();
            let id = runtime.next_object_id;
            runtime.next_object_id += 1;
            id
        };
        let reference = DexValue::Reference(std::rc::Rc::from(format!(
            "Ljava/io/InputStream;:raw/{resource_id}/{id}"
        )));
        self.runtime.borrow_mut().input_stream_values.insert(
            map_identity_key(&reference),
            InputStreamState { bytes, position: 0 },
        );
        Some(reference)
    }

    fn read_raw_resource_bytes(&self, resource_id: i32) -> Result<Vec<u8>> {
        if resource_id <= 0 {
            return Err(WinedroidError::new(format!(
                "openRawResource id must be positive\nresource id: {resource_id}"
            )));
        }
        let apk_path = self.apk_path.as_ref().ok_or_else(|| {
            WinedroidError::new("openRawResource requested but APK path is unavailable")
        })?;
        let resource = self
            .resolve_resource_file(resource_id as u32)?
            .ok_or_else(|| {
                WinedroidError::new(format!(
                    "openRawResource resource 0x{:08x} was not found in resources.arsc",
                    resource_id as u32
                ))
            })?;
        let mut apk = ApkArchive::open(apk_path)?;
        apk.read_required_file(&resource.path)
    }

    pub(in crate::dex_run) fn xml_new_pull_parser(&self) -> DexValue {
        let id = {
            let mut runtime = self.runtime.borrow_mut();
            let id = runtime.next_object_id;
            runtime.next_object_id += 1;
            id
        };
        let reference = DexValue::Reference(std::rc::Rc::from(format!(
            "Lorg/xmlpull/v1/XmlPullParser;:parser/{id}"
        )));
        self.runtime.borrow_mut().xml_parsers.insert(
            map_identity_key(&reference),
            XmlParserState {
                resource_id: 0,
                events: Vec::new(),
                position: XmlParserPosition::StartDocument,
                closed: false,
            },
        );
        reference
    }

    pub(in crate::dex_run) fn xml_pull_parser_set_input(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        let parser_key = map_identity_key(invocation.arguments.first()?);
        let stream_key = map_identity_key(invocation.arguments.get(1)?);
        let bytes = self
            .runtime
            .borrow()
            .input_stream_values
            .get(&stream_key)
            .map(|stream| stream.bytes[stream.position.min(stream.bytes.len())..].to_vec())?;
        let events = binary_xml::parse_plain_xml(&bytes).ok()?;
        self.runtime.borrow_mut().xml_parsers.insert(
            parser_key,
            XmlParserState {
                resource_id: 0,
                events,
                position: XmlParserPosition::StartDocument,
                closed: false,
            },
        );
        Some(())
    }

    pub(in crate::dex_run) fn record_menu_inflate(&self, invocation: &DexInvocation) -> Result<()> {
        if invocation.arguments.len() != 3 {
            return Err(WinedroidError::new(format!(
                "MenuInflater.inflate expected 3 arguments, got {}",
                invocation.arguments.len()
            )));
        }

        let DexValue::Int(resource_id) = invocation.arguments[1] else {
            return Err(WinedroidError::new(format!(
                "MenuInflater.inflate requires an integer menu resource ID\nargument: {}",
                invocation.arguments[1]
            )));
        };
        let menu = match &invocation.arguments[2] {
            DexValue::Object(_) | DexValue::Reference(_) => invocation.arguments[2].clone(),
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => {
                return Err(WinedroidError::new(format!(
                    "MenuInflater.inflate requires an object-backed Menu\nargument: {}",
                    invocation.arguments[2]
                )));
            }
        };
        let runtime_menu = self.parse_menu_resource(resource_id)?;
        let menu_key = map_identity_key(&menu);

        self.runtime
            .borrow_mut()
            .menus
            .insert(menu_key, runtime_menu);

        Ok(())
    }

    pub(in crate::dex_run) fn sync_inflated_menu_items(
        &self,
        invocation: &DexInvocation,
    ) -> Result<()> {
        if invocation.arguments.len() != 3 {
            return Ok(());
        }

        let menu = invocation.arguments[2].clone();
        let menu_key = map_identity_key(&menu);
        let parsed_items = {
            let runtime = self.runtime.borrow();
            let Some(menu) = runtime.menus.get(&menu_key) else {
                return Ok(());
            };
            menu.items.clone()
        };
        let item_values = self.menu_item_values_for_menu(&menu_key);

        for (item_value, parsed_item) in item_values.into_iter().zip(parsed_items) {
            self.write_menu_item_bootstrap_fields(&item_value, &parsed_item);
        }

        Ok(())
    }

    pub(in crate::dex_run) fn parse_menu_resource(&self, resource_id: i32) -> Result<RuntimeMenu> {
        let events = self.required_xml_resource_events(resource_id, "menu")?;
        let resources = self.resources.as_ref().ok_or_else(|| {
            WinedroidError::new("menu XML parser requested but APK resources.arsc is unavailable")
        })?;
        let items = runtime_menu_items_from_events_for_config(
            resource_id,
            &events,
            resources,
            self.resource_resolution_config(),
        )?;

        Ok(RuntimeMenu { resource_id, items })
    }

    pub(in crate::dex_run) fn required_xml_resource_events(
        &self,
        resource_id: i32,
        expected_type: &str,
    ) -> Result<Vec<XmlEvent>> {
        if resource_id <= 0 {
            return Err(WinedroidError::new(format!(
                "XML resource ID must be positive\nresource id: {resource_id}"
            )));
        }

        let apk_path = self.apk_path.as_ref().ok_or_else(|| {
            WinedroidError::new("XML resource requested but APK path is unavailable")
        })?;
        let resource_id_u32 = resource_id as u32;
        let resource = self
            .resolve_resource_file(resource_id_u32)?
            .ok_or_else(|| {
                WinedroidError::new(format!(
                    "XML resource 0x{resource_id_u32:08x} was not found in resources.arsc"
                ))
            })?;

        if resource.type_name != expected_type {
            return Err(WinedroidError::new(format!(
                "XML resource 0x{resource_id_u32:08x} resolved to `{}` but `{expected_type}` was required",
                resource.type_name
            )));
        }

        let mut apk = ApkArchive::open(apk_path)?;
        let data = apk.read_required_file(&resource.path)?;
        binary_xml::parse_binary_xml(&data).map_err(|error| {
            WinedroidError::with_source(
                format!(
                    "failed to parse XML resource 0x{resource_id_u32:08x} at `{}`",
                    resource.path
                ),
                error,
            )
        })
    }

    pub(in crate::dex_run) fn menu_item_values_for_menu(&self, menu_key: &str) -> Vec<DexValue> {
        let runtime = self.runtime.borrow();
        let mut candidate_lists = Vec::new();

        for ((owner_key, _field), value) in runtime.iter_fields() {
            if !owner_key.matches_key(menu_key) {
                continue;
            }

            let list_key = map_identity_key(value);
            if let Some(values) = runtime.list_entries.get(&list_key) {
                candidate_lists.push(values.clone());
            }
        }

        candidate_lists
            .into_iter()
            .max_by_key(Vec::len)
            .unwrap_or_default()
    }

    pub(in crate::dex_run) fn write_menu_item_bootstrap_fields(
        &self,
        item: &DexValue,
        parsed_item: &RuntimeMenuItem,
    ) {
        let Some(class_descriptor) = value_class_descriptor(item) else {
            return;
        };
        let item_id_field = DexFieldId {
            class_descriptor: class_descriptor.to_owned(),
            type_descriptor: "I".to_owned(),
            name: "a".to_owned(),
        };
        let title_field = DexFieldId {
            class_descriptor: class_descriptor.to_owned(),
            type_descriptor: "Ljava/lang/CharSequence;".to_owned(),
            name: "e".to_owned(),
        };
        let on_click_field = DexFieldId {
            class_descriptor: class_descriptor.to_owned(),
            type_descriptor: "Landroid/view/MenuItem$OnMenuItemClickListener;".to_owned(),
            name: "p".to_owned(),
        };

        self.write_stored_instance_field(item, &item_id_field, DexValue::Int(parsed_item.item_id));
        self.write_stored_instance_field(
            item,
            &title_field,
            DexValue::String(std::rc::Rc::from(parsed_item.title.clone())),
        );
        if let Some(method_name) = &parsed_item.on_click {
            self.write_stored_instance_field(
                item,
                &on_click_field,
                synthetic_menu_item_on_click_listener(method_name),
            );
        }
    }

    pub(in crate::dex_run) fn xml_pull_parser_event_type(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let parser_key = xml_parser_receiver_key(invocation)?;
        let runtime = self.runtime.borrow();
        let parser = runtime.xml_parsers.get(&parser_key)?;

        xml_parser_event_type(parser)
    }

    pub(in crate::dex_run) fn xml_pull_parser_next(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let parser_key = xml_parser_receiver_key(invocation)?;
        let mut runtime = self.runtime.borrow_mut();
        let parser = runtime.xml_parsers.get_mut(&parser_key)?;

        if parser.closed {
            return None;
        }

        parser.position = match parser.position {
            XmlParserPosition::StartDocument if parser.events.is_empty() => {
                XmlParserPosition::EndDocument
            }
            XmlParserPosition::StartDocument => XmlParserPosition::Event(0),
            XmlParserPosition::Event(index) if index + 1 < parser.events.len() => {
                XmlParserPosition::Event(index + 1)
            }
            XmlParserPosition::Event(_) | XmlParserPosition::EndDocument => {
                XmlParserPosition::EndDocument
            }
        };

        xml_parser_event_type(parser)
    }

    pub(in crate::dex_run) fn xml_pull_parser_name(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        let parser_key = xml_parser_receiver_key(invocation)?;
        let runtime = self.runtime.borrow();
        let parser = runtime.xml_parsers.get(&parser_key)?;

        if parser.closed {
            return None;
        }

        let XmlParserPosition::Event(index) = parser.position else {
            return Some(DexValue::Null);
        };
        let event = parser.events.get(index)?;
        let name = match event {
            XmlEvent::Start(element) => element.name.clone(),
            XmlEvent::End(element) => element.name.clone(),
        };

        Some(DexValue::String(std::rc::Rc::from(name)))
    }

    pub(in crate::dex_run) fn xml_pull_parser_depth(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        let parser_key = xml_parser_receiver_key(invocation)?;
        let runtime = self.runtime.borrow();
        let parser = runtime.xml_parsers.get(&parser_key)?;

        xml_parser_depth(parser)
    }

    pub(in crate::dex_run) fn xml_pull_parser_attribute_value(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 3 {
            return None;
        }

        let parser_key = xml_parser_receiver_key(invocation)?;
        let namespace = optional_string_argument(&invocation.arguments[1])?;
        let DexValue::String(name) = &invocation.arguments[2] else {
            return None;
        };
        let runtime = self.runtime.borrow();
        let parser = runtime.xml_parsers.get(&parser_key)?;

        if parser.closed {
            return None;
        }

        let XmlParserPosition::Event(index) = parser.position else {
            return Some(DexValue::Null);
        };
        let Some(XmlEvent::Start(element)) = parser.events.get(index) else {
            return Some(DexValue::Null);
        };
        let Some(attribute) = element.attributes.iter().find(|attribute| {
            attribute.name == **name
                && namespace.as_deref().map_or(true, |namespace| {
                    attribute.namespace.as_deref() == Some(namespace)
                })
        }) else {
            return Some(DexValue::Null);
        };

        Some(DexValue::String(std::rc::Rc::from(
            attribute.value.as_text(),
        )))
    }

    pub(in crate::dex_run) fn xml_resource_parser_close(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        let parser_key = xml_parser_receiver_key(invocation)?;
        let mut runtime = self.runtime.borrow_mut();
        let parser = runtime.xml_parsers.get_mut(&parser_key)?;

        parser.closed = true;

        Some(())
    }

    pub(in crate::dex_run) fn context_string_resource(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<DexValue>> {
        let Some(resource_id) = resources_int_resource_id(invocation) else {
            return Ok(None);
        };
        if resource_id <= 0 {
            return Err(invalid_resource_id_error("string", resource_id));
        }

        let text = self
            .resolve_string_resource_text(resource_id as u32)?
            .ok_or_else(|| resource_type_error("string", resource_id as u32))?;

        Ok(Some(DexValue::String(std::rc::Rc::from(text))))
    }

    pub(in crate::dex_run) fn resources_get_string_array(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<DexValue>> {
        let Some(resource_id) = resources_int_resource_id(invocation) else {
            return Ok(None);
        };
        if resource_id <= 0 {
            return Err(invalid_resource_id_error("string array", resource_id));
        }

        let values = self
            .resolve_text_array_resource(resource_id as u32)?
            .ok_or_else(|| resource_type_error("string array", resource_id as u32))?;
        let values = values
            .into_iter()
            .map(|value| {
                value
                    .map(|value| DexValue::String(value.into()))
                    .unwrap_or(DexValue::Null)
            })
            .collect();

        Ok(Some(self.allocate_filled_array(
            "[Ljava/lang/String;".to_owned(),
            values,
        )))
    }

    pub(in crate::dex_run) fn color_state_list_value_of(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        let DexValue::Int(color) = invocation.arguments[0] else {
            return None;
        };

        let color_state_list =
            self.allocate_object("Landroid/content/res/ColorStateList;".to_owned());
        self.write_stored_instance_field(
            &color_state_list,
            &color_state_list_default_color_field(),
            DexValue::Int(color),
        )
        .then_some(color_state_list)
    }

    pub(in crate::dex_run) fn color_state_list_get_default_color(
        &self,
        invocation: &DexInvocation,
    ) -> Option<i32> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        let DexValue::Int(color) = self.read_stored_instance_field(
            &invocation.arguments[0],
            &color_state_list_default_color_field(),
        )?
        else {
            return None;
        };

        Some(color)
    }
}

fn color_state_list_default_color_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/content/res/ColorStateList;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "defaultColor".to_owned(),
    }
}

pub(in crate::dex_run) fn context_drawable_resource(
    program: &DexProgram,
    invocation: &DexInvocation,
) -> Option<DexValue> {
    if invocation.arguments.len() != 2 {
        return None;
    }

    let DexValue::Int(resource_id) = invocation.arguments[1] else {
        return None;
    };

    Some(program.drawable_resource_value(resource_id as u32))
}

pub(in crate::dex_run) fn resources_drawable_resource(
    program: &DexProgram,
    invocation: &DexInvocation,
) -> Option<DexValue> {
    if !matches!(invocation.arguments.len(), 2 | 3) {
        return None;
    }

    let DexValue::Int(resource_id) = invocation.arguments[1] else {
        return None;
    };

    Some(program.drawable_resource_value(resource_id as u32))
}

pub(in crate::dex_run) fn drawable_resource_value(resource_id: u32) -> DexValue {
    DexValue::Reference(std::rc::Rc::from(format!(
        "Landroid/graphics/drawable/Drawable;:resource:{resource_id}"
    )))
}

impl DexProgram {
    pub(in crate::dex_run) fn drawable_resource_value(&self, resource_id: u32) -> DexValue {
        self.bitmap_drawable_resource_value(resource_id)
            .unwrap_or_else(|| drawable_resource_value(resource_id))
    }

    pub(in crate::dex_run) fn bitmap_drawable_get_bitmap(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        self.read_stored_instance_field(&invocation.arguments[0], &bitmap_drawable_bitmap_field())
    }

    fn bitmap_drawable_resource_value(&self, resource_id: u32) -> Option<DexValue> {
        let resource = self.resolve_resource_file(resource_id).ok()??;
        if !is_encoded_bitmap_resource_path(&resource.path) {
            return None;
        }

        let bytes = self.read_raw_resource_bytes(resource_id as i32).ok()?;
        let (width, height) = encoded_bitmap_dimensions(&bytes)?;
        let drawable =
            self.allocate_object("Landroid/graphics/drawable/BitmapDrawable;".to_owned());
        let bitmap = self.allocate_object("Landroid/graphics/Bitmap;".to_owned());

        // cite: frameworks-base BitmapFactory.java#decodeResource, decodeResourceStream
        // cite: frameworks-base BitmapDrawable.java#BitmapDrawable, getBitmap
        self.write_stored_instance_field(
            &bitmap,
            &bitmap_int_field("width"),
            DexValue::Int(width as i32),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            &bitmap,
            &bitmap_int_field("height"),
            DexValue::Int(height as i32),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            &bitmap,
            &bitmap_object_field("config", "Landroid/graphics/Bitmap$Config;"),
            DexValue::Reference(std::rc::Rc::from(
                "Landroid/graphics/Bitmap$Config;.ARGB_8888",
            )),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            &drawable,
            &bitmap_drawable_bitmap_field(),
            bitmap.clone(),
        )
        .then_some(())?;
        self.write_stored_instance_field(
            &drawable,
            &bitmap_drawable_resource_id_field(),
            DexValue::Int(resource_id as i32),
        )
        .then_some(())?;
        self.runtime
            .borrow_mut()
            .bitmap_encoded_images
            .insert(map_identity_key(&bitmap), bytes);

        Some(drawable)
    }
}

pub(in crate::dex_run) fn bitmap_drawable_bitmap_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/BitmapDrawable;".to_owned(),
        type_descriptor: "Landroid/graphics/Bitmap;".to_owned(),
        name: "bitmap".to_owned(),
    }
}

pub(in crate::dex_run) fn bitmap_drawable_resource_id_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/graphics/drawable/BitmapDrawable;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "winedroidSourceResourceId".to_owned(),
    }
}

fn is_encoded_bitmap_resource_path(path: &str) -> bool {
    path.rsplit('.').next().is_some_and(|extension| {
        matches!(
            extension.to_ascii_lowercase().as_str(),
            "png" | "jpg" | "jpeg" | "webp"
        )
    })
}

fn encoded_bitmap_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    png_dimensions(bytes)
        .or_else(|| jpeg_dimensions(bytes))
        .or_else(|| webp_dimensions(bytes))
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    const PNG_SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if bytes.len() < 24 || bytes[0..8] != PNG_SIGNATURE {
        return None;
    }

    positive_dimensions(
        u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]),
        u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]),
    )
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 4 || bytes[0] != 0xff || bytes[1] != 0xd8 {
        return None;
    }

    let mut offset = 2;
    while offset + 3 < bytes.len() {
        while offset < bytes.len() && bytes[offset] != 0xff {
            offset += 1;
        }
        while offset < bytes.len() && bytes[offset] == 0xff {
            offset += 1;
        }
        if offset >= bytes.len() {
            return None;
        }

        let marker = bytes[offset];
        offset += 1;
        if matches!(marker, 0xd8 | 0xd9 | 0x01) || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        if offset + 1 >= bytes.len() {
            return None;
        }

        let segment_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        if segment_len < 2 || offset + segment_len > bytes.len() {
            return None;
        }
        if is_jpeg_start_of_frame(marker) && segment_len >= 7 {
            let height = u16::from_be_bytes([bytes[offset + 3], bytes[offset + 4]]) as u32;
            let width = u16::from_be_bytes([bytes[offset + 5], bytes[offset + 6]]) as u32;
            return positive_dimensions(width, height);
        }

        offset += segment_len;
    }

    None
}

fn is_jpeg_start_of_frame(marker: u8) -> bool {
    matches!(
        marker,
        0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7 | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf
    )
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.len() < 30 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return None;
    }

    match &bytes[12..16] {
        b"VP8X" if bytes.len() >= 30 => {
            let width = 1
                + u32::from(bytes[24])
                + (u32::from(bytes[25]) << 8)
                + (u32::from(bytes[26]) << 16);
            let height = 1
                + u32::from(bytes[27])
                + (u32::from(bytes[28]) << 8)
                + (u32::from(bytes[29]) << 16);
            positive_dimensions(width, height)
        }
        b"VP8 " if bytes.len() >= 30 && bytes[23..26] == [0x9d, 0x01, 0x2a] => {
            let width = u16::from_le_bytes([bytes[26], bytes[27]]) as u32 & 0x3fff;
            let height = u16::from_le_bytes([bytes[28], bytes[29]]) as u32 & 0x3fff;
            positive_dimensions(width, height)
        }
        b"VP8L" if bytes.len() >= 25 && bytes[20] == 0x2f => {
            let bits = u32::from(bytes[21])
                | (u32::from(bytes[22]) << 8)
                | (u32::from(bytes[23]) << 16)
                | (u32::from(bytes[24]) << 24);
            let width = (bits & 0x3fff) + 1;
            let height = ((bits >> 14) & 0x3fff) + 1;
            positive_dimensions(width, height)
        }
        _ => None,
    }
}

fn positive_dimensions(width: u32, height: u32) -> Option<(u32, u32)> {
    (width > 0 && height > 0 && width <= i32::MAX as u32 && height <= i32::MAX as u32)
        .then_some((width, height))
}

pub(in crate::dex_run) fn view_tree_observer_reference(
    invocation: &DexInvocation,
) -> Option<DexValue> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {
            Some(DexValue::Reference(std::rc::Rc::from(format!(
                "Landroid/view/ViewTreeObserver;:{}",
                map_identity_key(&invocation.arguments[0])
            ))))
        }
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

pub(in crate::dex_run) fn measure_spec_make(invocation: &DexInvocation) -> Option<i32> {
    if invocation.arguments.len() != 2 {
        return None;
    }

    let DexValue::Int(size) = invocation.arguments[0] else {
        return None;
    };
    let DexValue::Int(mode) = invocation.arguments[1] else {
        return None;
    };

    let value = (size & 0x3fff_ffff) | (mode & !0x3fff_ffff);

    Some(value)
}

pub(in crate::dex_run) fn measure_spec_part(invocation: &DexInvocation, part: &str) -> Option<i32> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    let DexValue::Int(spec) = invocation.arguments[0] else {
        return None;
    };

    match part {
        "getMode" => Some(spec & !0x3fff_ffff),
        "getSize" => Some(spec & 0x3fff_ffff),
        _ => None,
    }
}

pub(in crate::dex_run) fn measure_spec_size(spec: i32) -> i32 {
    spec & 0x3fff_ffff
}

// cite: frameworks-base View.java#MeasureSpec
pub(in crate::dex_run) fn measure_spec_is_exactly(spec: i32) -> bool {
    (spec as u32) & 0xc000_0000 == 0x4000_0000
}

pub(in crate::dex_run) fn resource_name(invocation: &DexInvocation) -> Option<String> {
    if invocation.arguments.len() != 2 {
        return None;
    }

    let DexValue::Int(resource_id) = invocation.arguments[1] else {
        return None;
    };

    Some(format!("resource:{resource_id}"))
}

#[cfg(test)]
pub(in crate::dex_run) fn xml_resource_parser_reference(resource_id: i32) -> DexValue {
    DexValue::Reference(std::rc::Rc::from(format!(
        "Landroid/content/res/XmlResourceParser;:layout/{resource_id}"
    )))
}

fn optional_string_argument(value: &DexValue) -> Option<Option<String>> {
    match value {
        DexValue::Null => Some(None),
        DexValue::String(value) if value.is_empty() => Some(None),
        DexValue::String(value) => Some(Some(value.clone().to_string())),
        DexValue::Uninitialized
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::Object(_)
        | DexValue::Reference(_) => None,
    }
}

pub(in crate::dex_run) fn xml_parser_receiver_key(invocation: &DexInvocation) -> Option<String> {
    if invocation.arguments.is_empty() {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {
            Some(map_identity_key(&invocation.arguments[0]))
        }
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

pub(in crate::dex_run) fn xml_parser_event_type(parser: &XmlParserState) -> Option<i32> {
    if parser.closed {
        return None;
    }

    match parser.position {
        XmlParserPosition::StartDocument => Some(XML_PULL_START_DOCUMENT),
        XmlParserPosition::EndDocument => Some(XML_PULL_END_DOCUMENT),
        XmlParserPosition::Event(index) => match parser.events.get(index)? {
            XmlEvent::Start(_) => Some(XML_PULL_START_TAG),
            XmlEvent::End(_) => Some(XML_PULL_END_TAG),
        },
    }
}

pub(in crate::dex_run) fn xml_parser_depth(parser: &XmlParserState) -> Option<i32> {
    if parser.closed {
        return None;
    }

    let XmlParserPosition::Event(current_index) = parser.position else {
        return Some(0);
    };

    let mut depth = 0_i32;

    for (index, event) in parser.events.iter().enumerate() {
        if index > current_index {
            break;
        }

        match event {
            XmlEvent::Start(_) => {
                depth = depth.saturating_add(1);
                if index == current_index {
                    return Some(depth);
                }
            }
            XmlEvent::End(_) => {
                if index == current_index {
                    return Some(depth);
                }
                depth = depth.saturating_sub(1);
            }
        }
    }

    Some(0)
}

#[cfg(test)]
pub(in crate::dex_run) fn runtime_menu_items_from_events(
    resource_id: i32,
    events: &[XmlEvent],
    resources: &[u8],
) -> Result<Vec<RuntimeMenuItem>> {
    runtime_menu_items_from_events_for_config(
        resource_id,
        events,
        resources,
        resource_table::ResourceResolutionConfig::default(),
    )
}

fn runtime_menu_items_from_events_for_config(
    resource_id: i32,
    events: &[XmlEvent],
    resources: &[u8],
    config: resource_table::ResourceResolutionConfig,
) -> Result<Vec<RuntimeMenuItem>> {
    let mut items = Vec::new();

    for event in events {
        let XmlEvent::Start(element) = event else {
            continue;
        };
        if element.name != "item" {
            continue;
        }

        let position = items.len();
        let item_id =
            menu_item_id(element).unwrap_or_else(|| synthetic_menu_item_id(resource_id, position));
        let title = menu_item_title_for_config(element, resources, config)?;
        let enabled = menu_item_bool_attribute(element, "enabled").unwrap_or(true);
        let visible = menu_item_bool_attribute(element, "visible").unwrap_or(true);
        let on_click = menu_item_on_click(element)?;
        let icon_resource = menu_item_icon(element);
        let show_as_action = menu_item_show_as_action(element);

        items.push(RuntimeMenuItem {
            item_id,
            title,
            enabled,
            visible,
            on_click,
            icon_resource,
            show_as_action,
        });
    }

    Ok(items)
}

pub(in crate::dex_run) fn menu_item_id(element: &binary_xml::XmlStartElement) -> Option<i32> {
    let attribute = element.attribute("id")?;

    match &attribute.value {
        binary_xml::XmlValue::Reference(value) | binary_xml::XmlValue::Hex(value) => {
            i32::try_from(*value).ok()
        }
        binary_xml::XmlValue::Integer(value) => Some(*value),
        _ => None,
    }
}

pub(in crate::dex_run) fn synthetic_menu_item_id(resource_id: i32, position: usize) -> i32 {
    let resource_component = resource_id & 0x00ff_ffff;
    let position_component = i32::try_from(position).unwrap_or(i32::MAX - 1);

    -resource_component
        .saturating_add(position_component)
        .saturating_add(1)
}

fn menu_item_title_for_config(
    element: &binary_xml::XmlStartElement,
    resources: &[u8],
    config: resource_table::ResourceResolutionConfig,
) -> Result<String> {
    let Some(attribute) = element.attribute("title") else {
        return Ok(String::new());
    };

    match &attribute.value {
        binary_xml::XmlValue::String(value) => Ok(value.clone()),
        binary_xml::XmlValue::Reference(resource_id) => {
            // cite: frameworks-base MenuInflater.java#readItem
            resolve_menu_string_resource_for_config(resources, *resource_id, config)
        }
        binary_xml::XmlValue::Null => Ok(String::new()),
        value => Err(WinedroidError::new(format!(
            "menu item title has unsupported value `{value}`"
        ))),
    }
}

fn resolve_menu_string_resource_for_config(
    resources: &[u8],
    resource_id: u32,
    config: resource_table::ResourceResolutionConfig,
) -> Result<String> {
    // cite: frameworks-base ResourceTypes.cpp#ResTable::getResource, ResTable::resolveReference
    let Some(resource) = resource_table::resolve_resource_value_references_for_config(
        resources,
        resource_id,
        config,
    )?
    else {
        return Err(WinedroidError::new(format!(
            "menu title string resource 0x{resource_id:08x} was not found"
        )));
    };

    match resource.value {
        resource_table::ResourcePrimitive::String(value) => Ok(value),
        value => Err(WinedroidError::new(format!(
            "menu title string resource 0x{resource_id:08x} has unsupported value {value:?}"
        ))),
    }
}

pub(in crate::dex_run) fn menu_item_icon(element: &binary_xml::XmlStartElement) -> Option<i32> {
    let attribute = element.attribute("icon")?;
    match &attribute.value {
        binary_xml::XmlValue::Reference(value) | binary_xml::XmlValue::Hex(value) => {
            i32::try_from(*value).ok()
        }
        binary_xml::XmlValue::Integer(value) => Some(*value),
        _ => None,
    }
}

pub(in crate::dex_run) fn menu_item_show_as_action(element: &binary_xml::XmlStartElement) -> i32 {
    let Some(attribute) = element.attribute("showAsAction") else {
        return 0;
    };
    match &attribute.value {
        binary_xml::XmlValue::Integer(value) => *value,
        binary_xml::XmlValue::Hex(value) => *value as i32,
        _ => 0,
    }
}

pub(in crate::dex_run) fn menu_item_bool_attribute(
    element: &binary_xml::XmlStartElement,
    name: &str,
) -> Option<bool> {
    element
        .attribute(name)
        .and_then(|attribute| attribute.value.as_bool())
}

pub(in crate::dex_run) fn menu_item_on_click(
    element: &binary_xml::XmlStartElement,
) -> Result<Option<String>> {
    let Some(attribute) = element.attribute("onClick") else {
        return Ok(None);
    };

    match &attribute.value {
        binary_xml::XmlValue::String(value) if value.is_empty() => Ok(None),
        binary_xml::XmlValue::String(value) => Ok(Some(value.clone())),
        binary_xml::XmlValue::Null => Ok(None),
        value => Err(WinedroidError::new(format!(
            "menu item onClick has unsupported value `{value}`"
        ))),
    }
}

const MENU_ITEM_ON_CLICK_LISTENER_PREFIX: &str =
    "Landroid/view/MenuItem$OnMenuItemClickListener;:xml-on-click/";

pub(in crate::dex_run) fn synthetic_menu_item_on_click_listener(method_name: &str) -> DexValue {
    DexValue::Reference(std::rc::Rc::from(format!(
        "{MENU_ITEM_ON_CLICK_LISTENER_PREFIX}{method_name}"
    )))
}

pub(in crate::dex_run) fn menu_item_on_click_listener_method_name(
    value: &DexValue,
) -> Option<&str> {
    let DexValue::Reference(reference) = value else {
        return None;
    };

    reference.strip_prefix(MENU_ITEM_ON_CLICK_LISTENER_PREFIX)
}

pub(in crate::dex_run) fn xml_as_attribute_set(invocation: &DexInvocation) -> Option<DexValue> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => Some(invocation.arguments[0].clone()),
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

pub(in crate::dex_run) fn resources_int_resource_id(invocation: &DexInvocation) -> Option<i32> {
    if !matches!(invocation.arguments.len(), 2 | 3) {
        return None;
    }

    let DexValue::Int(resource_id) = invocation.arguments[1] else {
        return None;
    };

    Some(resource_id)
}

pub(in crate::dex_run) fn context_system_service(invocation: &DexInvocation) -> Option<DexValue> {
    if invocation.arguments.len() != 2 {
        return None;
    }

    let DexValue::String(service_name) = &invocation.arguments[1] else {
        return None;
    };
    let class_descriptor = system_service_runtime_descriptor(service_name)
        .unwrap_or("Landroid/content/SystemService;");

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "{class_descriptor}:{service_name}"
    ))))
}

pub(in crate::dex_run) fn context_system_service_by_class(
    invocation: &DexInvocation,
) -> Option<DexValue> {
    if invocation.arguments.len() != 2 {
        return None;
    }

    let class_descriptor = class_reference_descriptor_from_value(&invocation.arguments[1])?;
    let service_name = system_service_name_for_class(class_descriptor)?;
    // cite: frameworks-base Context.java#getSystemService
    let runtime_descriptor =
        system_service_runtime_descriptor(service_name).unwrap_or(class_descriptor);

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "{runtime_descriptor}:{service_name}"
    ))))
}

// cite: frameworks-base SystemServiceRegistry.java#ACCESSIBILITY_SERVICE, AccessibilityManager, AUTOFILL_MANAGER_SERVICE, AutofillManager, CLIPBOARD_SERVICE, ClipboardManager, INPUT_METHOD_SERVICE, InputMethodManager, LAYOUT_INFLATER_SERVICE, LayoutInflater, VIBRATOR_MANAGER_SERVICE, VibratorManager, SystemVibratorManager, WINDOW_SERVICE, WindowManager, WindowManagerImpl
// cite: frameworks-base Context.java#getSystemService, CONNECTIVITY_SERVICE
fn system_service_runtime_descriptor(service_name: &str) -> Option<&'static str> {
    match service_name {
        "accessibility" => Some("Landroid/view/accessibility/AccessibilityManager;"),
        "autofill" => Some("Landroid/view/autofill/AutofillManager;"),
        "clipboard" => Some("Landroid/content/ClipboardManager;"),
        "connectivity" => Some("Landroid/net/ConnectivityManager;"),
        "input_method" => Some("Landroid/view/inputmethod/InputMethodManager;"),
        "layout_inflater" => Some("Landroid/view/LayoutInflater;"),
        "vibrator_manager" => Some("Landroid/os/SystemVibratorManager;"),
        "window" => Some("Landroid/view/WindowManagerImpl;"),
        _ => None,
    }
}

// cite: frameworks-base SystemServiceRegistry.java#registerService
// cite: frameworks-base Context.java#CLIPBOARD_SERVICE, CONNECTIVITY_SERVICE, VIBRATOR_SERVICE, VIBRATOR_MANAGER_SERVICE
fn system_service_name_for_class(class_descriptor: &str) -> Option<&'static str> {
    match class_descriptor {
        "Landroid/content/ClipboardManager;" | "Landroid/text/ClipboardManager;" => {
            Some("clipboard")
        }
        "Landroid/net/ConnectivityManager;" => Some("connectivity"),
        "Landroid/view/autofill/AutofillManager;" => Some("autofill"),
        "Landroid/view/inputmethod/InputMethodManager;" => Some("input_method"),
        "Landroid/view/LayoutInflater;" => Some("layout_inflater"),
        "Landroid/os/Vibrator;" => Some("vibrator"),
        "Landroid/os/VibratorManager;" => Some("vibrator_manager"),
        "Landroid/view/WindowManager;" => Some("window"),
        _ => None,
    }
}

impl DexProgram {
    pub(in crate::dex_run) fn context_theme_wrapper_init(
        &self,
        invocation: &DexInvocation,
    ) -> Option<()> {
        if invocation.arguments.len() != 3 {
            return None;
        }

        match &invocation.arguments[0] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        }

        match &invocation.arguments[1] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        }

        let DexValue::Int(theme_resource_id) = invocation.arguments[2] else {
            return None;
        };

        self.write_stored_instance_field(
            &invocation.arguments[0],
            &context_wrapper_base_field(),
            invocation.arguments[1].clone(),
        );
        self.write_stored_instance_field(
            &invocation.arguments[0],
            &context_theme_wrapper_theme_resource_field(),
            DexValue::Int(theme_resource_id),
        );

        Some(())
    }

    pub(in crate::dex_run) fn context_wrapper_base_context(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 1 {
            return None;
        }

        match &invocation.arguments[0] {
            DexValue::Object(_) | DexValue::Reference(_) => {}
            DexValue::Uninitialized
            | DexValue::Null
            | DexValue::Int(_)
            | DexValue::Long(_)
            | DexValue::Float(_)
            | DexValue::String(_) => return None,
        }

        let value = self
            .read_stored_instance_field(&invocation.arguments[0], &context_wrapper_base_field())
            .unwrap_or(DexValue::Null);

        Some(value)
    }
}

impl DexProgram {
    pub(in crate::dex_run) fn context_obtain_styled_attributes(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 2 {
            return None;
        }
        if !dex_value_is_object_like(&invocation.arguments[0])
            || !dex_value_is_object_like(&invocation.arguments[1])
        {
            return None;
        }

        let theme = self.ensure_context_theme(&invocation.arguments[0])?;
        self.theme_obtain_styled_attributes_from_value(&theme, &invocation.arguments[1])
    }

    pub(in crate::dex_run) fn theme_obtain_styled_attributes(
        &self,
        invocation: &DexInvocation,
    ) -> Option<DexValue> {
        if invocation.arguments.len() != 2 {
            return None;
        }
        if !dex_value_is_object_like(&invocation.arguments[0])
            || !dex_value_is_object_like(&invocation.arguments[1])
        {
            return None;
        }

        self.theme_obtain_styled_attributes_from_value(
            &invocation.arguments[0],
            &invocation.arguments[1],
        )
    }

    fn theme_obtain_styled_attributes_from_value(
        &self,
        theme: &DexValue,
        styleable: &DexValue,
    ) -> Option<DexValue> {
        let typed_array = DexValue::Reference(std::rc::Rc::from(format!(
            "Landroid/content/res/TypedArray;:{}",
            map_identity_key(styleable)
        )));
        let values = self.resolve_theme_styled_attribute_values(theme, styleable);
        if !values.is_empty() {
            let typed_array_key = map_identity_key(&typed_array);
            self.runtime
                .borrow_mut()
                .typed_arrays
                .insert(typed_array_key, RuntimeTypedArray { values });
        }

        Some(typed_array)
    }

    fn resolve_theme_styled_attribute_values(
        &self,
        theme: &DexValue,
        styleable: &DexValue,
    ) -> HashMap<i32, resource_table::ResourcePrimitive> {
        let mut values = HashMap::new();
        let Some(entries) = self.styleable_entries(styleable) else {
            return values;
        };

        for (index, value) in entries.iter().enumerate() {
            let Some(attribute_id) = Self::styleable_attribute_resource_id(value) else {
                continue;
            };
            let Some(resolved) = self.resolve_theme_attribute(theme, attribute_id) else {
                continue;
            };
            let Ok(index) = i32::try_from(index) else {
                continue;
            };
            values.insert(index, resolved);
        }

        values
    }

    fn styleable_entries(&self, styleable: &DexValue) -> Option<Vec<DexValue>> {
        let styleable_key = map_identity_key(styleable);
        self.runtime
            .borrow()
            .array_entries
            .get(&styleable_key)
            .cloned()
    }
}

pub(in crate::dex_run) fn context_obtain_attribute_set_styled_attributes(
    invocation: &DexInvocation,
) -> Option<DexValue> {
    if !matches!(invocation.arguments.len(), 3 | 5) {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {}
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => return None,
    }

    match &invocation.arguments[1] {
        DexValue::Null | DexValue::Int(0) | DexValue::Object(_) | DexValue::Reference(_) => {}
        DexValue::Uninitialized | DexValue::Long(_) | DexValue::Float(_) | DexValue::String(_) => {
            return None
        }
        DexValue::Int(_) => return None,
    }

    match &invocation.arguments[2] {
        DexValue::Object(_) | DexValue::Reference(_) => {}
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => return None,
    }

    if invocation.arguments.len() == 5 {
        let DexValue::Int(_) = invocation.arguments[3] else {
            return None;
        };
        let DexValue::Int(_) = invocation.arguments[4] else {
            return None;
        };
    }

    if invocation.arguments.len() == 5 {
        return Some(DexValue::Reference(std::rc::Rc::from(format!(
            "Landroid/content/res/TypedArray;:{}:{}:{}:{}",
            map_identity_key(&invocation.arguments[1]),
            map_identity_key(&invocation.arguments[2]),
            invocation.arguments[3],
            invocation.arguments[4]
        ))));
    }

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "Landroid/content/res/TypedArray;:{}:{}",
        map_identity_key(&invocation.arguments[1]),
        map_identity_key(&invocation.arguments[2])
    ))))
}

pub(in crate::dex_run) fn typed_array_index(invocation: &DexInvocation) -> Option<i32> {
    if invocation.arguments.len() != 2 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {}
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => return None,
    }

    let DexValue::Int(index) = invocation.arguments[1] else {
        return None;
    };

    Some(index)
}

pub(in crate::dex_run) fn typed_array_receiver(invocation: &DexInvocation) -> Option<()> {
    if invocation.arguments.len() != 1 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => Some(()),
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

pub(in crate::dex_run) fn typed_array_reference_contains_key(
    typed_array_key: &str,
    nested_key: &str,
) -> bool {
    typed_array_key == nested_key
        || typed_array_key.contains(&format!(":{nested_key}:"))
        || typed_array_key.ends_with(&format!(":{nested_key}"))
}

fn resource_primitive_resource_id(value: &resource_table::ResourcePrimitive) -> Option<i32> {
    match value {
        resource_table::ResourcePrimitive::Reference(resource_id)
        | resource_table::ResourcePrimitive::Attribute(resource_id) => Some(*resource_id as i32),
        resource_table::ResourcePrimitive::String(_)
        | resource_table::ResourcePrimitive::Float(_)
        | resource_table::ResourcePrimitive::Dimension(_)
        | resource_table::ResourcePrimitive::Fraction(_)
        | resource_table::ResourcePrimitive::Integer(_)
        | resource_table::ResourcePrimitive::Hex(_)
        | resource_table::ResourcePrimitive::Boolean(_)
        | resource_table::ResourcePrimitive::Color(_)
        | resource_table::ResourcePrimitive::Array(_)
        | resource_table::ResourcePrimitive::Bag(_)
        | resource_table::ResourcePrimitive::Unknown { .. } => None,
    }
}

// cite: frameworks-base TypedValue.java#coerceToString
fn resource_primitive_coerce_to_string(
    value: &resource_table::ResourcePrimitive,
) -> Option<String> {
    match value {
        resource_table::ResourcePrimitive::String(value) => Some(value.clone()),
        resource_table::ResourcePrimitive::Reference(value) => Some(format!("@{value}")),
        resource_table::ResourcePrimitive::Attribute(value) => Some(format!("?{value}")),
        resource_table::ResourcePrimitive::Float(value) => {
            java_float_to_string(f32::from_bits(*value))
        }
        resource_table::ResourcePrimitive::Dimension(value) => {
            let unit = ["px", "dip", "sp", "pt", "in", "mm"].get((value & 0x0f) as usize)?;
            java_float_to_string(android_complex_to_float(*value))
                .map(|value| format!("{value}{unit}"))
        }
        resource_table::ResourcePrimitive::Fraction(value) => {
            let unit = ["%", "%p"].get((value & 0x0f) as usize)?;
            java_float_to_string(android_complex_to_float(*value) * 100.0)
                .map(|value| format!("{value}{unit}"))
        }
        resource_table::ResourcePrimitive::Integer(value) => Some(value.to_string()),
        resource_table::ResourcePrimitive::Hex(value) => Some(format!("0x{value:x}")),
        resource_table::ResourcePrimitive::Boolean(value) => Some(value.to_string()),
        resource_table::ResourcePrimitive::Color(value) => Some(format!("#{value:x}")),
        resource_table::ResourcePrimitive::Unknown { data_type, data }
            if (0x10..=0x1f).contains(data_type) =>
        {
            Some((*data as i32).to_string())
        }
        resource_table::ResourcePrimitive::Array(_)
        | resource_table::ResourcePrimitive::Bag(_)
        | resource_table::ResourcePrimitive::Unknown { .. } => None,
    }
}

fn resource_primitive_bool(value: &resource_table::ResourcePrimitive) -> Option<bool> {
    match value {
        resource_table::ResourcePrimitive::Boolean(value) => Some(*value),
        resource_table::ResourcePrimitive::Integer(value) => Some(*value != 0),
        resource_table::ResourcePrimitive::Hex(value) => Some(*value != 0),
        resource_table::ResourcePrimitive::String(value) => match value.as_str() {
            "true" => Some(true),
            "false" => Some(false),
            _ => None,
        },
        resource_table::ResourcePrimitive::Reference(_)
        | resource_table::ResourcePrimitive::Attribute(_)
        | resource_table::ResourcePrimitive::Float(_)
        | resource_table::ResourcePrimitive::Dimension(_)
        | resource_table::ResourcePrimitive::Fraction(_)
        | resource_table::ResourcePrimitive::Color(_)
        | resource_table::ResourcePrimitive::Array(_)
        | resource_table::ResourcePrimitive::Bag(_)
        | resource_table::ResourcePrimitive::Unknown { .. } => None,
    }
}

fn resource_primitive_int(value: &resource_table::ResourcePrimitive) -> Option<i32> {
    match value {
        resource_table::ResourcePrimitive::String(value) => xml_string_int(value),
        value => resource_primitive_raw_int(value),
    }
}

fn resource_primitive_raw_int(value: &resource_table::ResourcePrimitive) -> Option<i32> {
    match value {
        resource_table::ResourcePrimitive::Integer(value) => Some(*value),
        resource_table::ResourcePrimitive::Hex(value)
        | resource_table::ResourcePrimitive::Color(value) => Some(*value as i32),
        resource_table::ResourcePrimitive::Boolean(value) => Some(if *value { -1 } else { 0 }),
        resource_table::ResourcePrimitive::Reference(_)
        | resource_table::ResourcePrimitive::Attribute(_)
        | resource_table::ResourcePrimitive::String(_)
        | resource_table::ResourcePrimitive::Float(_)
        | resource_table::ResourcePrimitive::Dimension(_)
        | resource_table::ResourcePrimitive::Fraction(_)
        | resource_table::ResourcePrimitive::Array(_)
        | resource_table::ResourcePrimitive::Bag(_)
        | resource_table::ResourcePrimitive::Unknown { .. } => None,
    }
}

fn resource_primitive_integer_family_data(
    value: &resource_table::ResourcePrimitive,
) -> Option<u32> {
    match value {
        resource_table::ResourcePrimitive::Integer(value) => Some(*value as u32),
        resource_table::ResourcePrimitive::Hex(value)
        | resource_table::ResourcePrimitive::Color(value) => Some(*value),
        resource_table::ResourcePrimitive::Boolean(value) => {
            Some(if *value { u32::MAX } else { 0 })
        }
        resource_table::ResourcePrimitive::Unknown { data_type, data }
            if (0x10..=0x1f).contains(data_type) =>
        {
            Some(*data)
        }
        resource_table::ResourcePrimitive::Reference(_)
        | resource_table::ResourcePrimitive::Attribute(_)
        | resource_table::ResourcePrimitive::String(_)
        | resource_table::ResourcePrimitive::Float(_)
        | resource_table::ResourcePrimitive::Dimension(_)
        | resource_table::ResourcePrimitive::Fraction(_)
        | resource_table::ResourcePrimitive::Array(_)
        | resource_table::ResourcePrimitive::Bag(_)
        | resource_table::ResourcePrimitive::Unknown { .. } => None,
    }
}

fn resource_primitive_float(value: &resource_table::ResourcePrimitive) -> Option<f32> {
    match value {
        resource_table::ResourcePrimitive::Float(value) => Some(f32::from_bits(*value)),
        resource_table::ResourcePrimitive::Dimension(value)
        | resource_table::ResourcePrimitive::Fraction(value) => {
            Some(android_complex_dimension_to_px_float(*value))
        }
        resource_table::ResourcePrimitive::Integer(value) => Some(*value as f32),
        resource_table::ResourcePrimitive::Hex(value) => Some(*value as f32),
        resource_table::ResourcePrimitive::String(value) => value.parse().ok(),
        resource_table::ResourcePrimitive::Reference(_)
        | resource_table::ResourcePrimitive::Attribute(_)
        | resource_table::ResourcePrimitive::Boolean(_)
        | resource_table::ResourcePrimitive::Color(_)
        | resource_table::ResourcePrimitive::Array(_)
        | resource_table::ResourcePrimitive::Bag(_)
        | resource_table::ResourcePrimitive::Unknown { .. } => None,
    }
}

pub(in crate::dex_run) fn typed_array_default_boolean(invocation: &DexInvocation) -> Option<i32> {
    if invocation.arguments.len() != 3 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {}
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => return None,
    }

    let DexValue::Int(index) = invocation.arguments[1] else {
        return None;
    };
    let DexValue::Int(default_value) = invocation.arguments[2] else {
        return None;
    };

    if index == 117 {
        return Some(1);
    }

    Some(default_value)
}

pub(in crate::dex_run) fn typed_array_index_and_default_int(
    invocation: &DexInvocation,
) -> Option<(i32, i32)> {
    if invocation.arguments.len() != 3 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {}
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => return None,
    }

    let DexValue::Int(index) = invocation.arguments[1] else {
        return None;
    };
    let DexValue::Int(default_value) = invocation.arguments[2] else {
        return None;
    };

    Some((index, default_value))
}

fn typed_array_index_and_default_float(invocation: &DexInvocation) -> Option<(i32, f32)> {
    if invocation.arguments.len() != 3 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {}
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => return None,
    }

    let DexValue::Int(index) = invocation.arguments[1] else {
        return None;
    };
    let default_value = dex_float_value(&invocation.arguments[2])?;

    Some((index, default_value))
}

fn dex_float_value(value: &DexValue) -> Option<f32> {
    match value {
        DexValue::Float(value) => Some(*value),
        DexValue::Int(value) => Some(f32::from_bits(*value as u32)),
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Long(_)
        | DexValue::String(_)
        | DexValue::Object(_)
        | DexValue::Reference(_) => None,
    }
}

pub(in crate::dex_run) fn xml_value_resource_id(value: &binary_xml::XmlValue) -> Option<i32> {
    match value {
        binary_xml::XmlValue::Reference(0) => None,
        binary_xml::XmlValue::Reference(resource_id) | binary_xml::XmlValue::Hex(resource_id) => {
            i32::try_from(*resource_id).ok()
        }
        binary_xml::XmlValue::Integer(resource_id) => Some(*resource_id),
        binary_xml::XmlValue::Null
        | binary_xml::XmlValue::String(_)
        | binary_xml::XmlValue::Boolean(_)
        | binary_xml::XmlValue::AttributeReference(_)
        | binary_xml::XmlValue::Float(_)
        | binary_xml::XmlValue::Dimension(_)
        | binary_xml::XmlValue::Fraction(_)
        | binary_xml::XmlValue::Color(_)
        | binary_xml::XmlValue::Unknown { .. } => None,
    }
}

fn xml_value_float(value: &binary_xml::XmlValue) -> Option<f32> {
    match value {
        binary_xml::XmlValue::Float(value) => Some(f32::from_bits(*value)),
        binary_xml::XmlValue::Dimension(value) => {
            Some(android_complex_dimension_to_px_float(*value))
        }
        binary_xml::XmlValue::Integer(value) => Some(*value as f32),
        binary_xml::XmlValue::Hex(value) => Some(*value as f32),
        binary_xml::XmlValue::String(value) => value.parse().ok(),
        binary_xml::XmlValue::Null
        | binary_xml::XmlValue::Boolean(_)
        | binary_xml::XmlValue::Reference(_)
        | binary_xml::XmlValue::AttributeReference(_)
        | binary_xml::XmlValue::Fraction(_)
        | binary_xml::XmlValue::Color(_)
        | binary_xml::XmlValue::Unknown { .. } => None,
    }
}

// cite: frameworks-base TypedArray.java#getInt
fn xml_value_int(value: &binary_xml::XmlValue) -> Option<i32> {
    match value {
        binary_xml::XmlValue::String(value) => xml_string_int(value),
        value => xml_value_raw_int(value),
    }
}

fn xml_value_raw_int(value: &binary_xml::XmlValue) -> Option<i32> {
    match value {
        binary_xml::XmlValue::Integer(value) => Some(*value),
        binary_xml::XmlValue::Hex(value) | binary_xml::XmlValue::Color(value) => {
            Some(*value as i32)
        }
        binary_xml::XmlValue::Boolean(value) => Some(if *value { -1 } else { 0 }),
        binary_xml::XmlValue::Null
        | binary_xml::XmlValue::String(_)
        | binary_xml::XmlValue::Reference(_)
        | binary_xml::XmlValue::AttributeReference(_)
        | binary_xml::XmlValue::Float(_)
        | binary_xml::XmlValue::Dimension(_)
        | binary_xml::XmlValue::Fraction(_)
        | binary_xml::XmlValue::Unknown { .. } => None,
    }
}

// cite: frameworks-base XmlUtils.java#convertValueToInt
fn xml_string_int(value: &str) -> Option<i32> {
    if value.is_empty() {
        return None;
    }

    let (sign, value) = value
        .strip_prefix('-')
        .map_or((1, value), |value| (-1, value));
    if value.is_empty() {
        return None;
    }

    let (radix, digits) = if value == "0" {
        return Some(0);
    } else if let Some(value) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        (16, value)
    } else if let Some(value) = value.strip_prefix('0') {
        (8, value)
    } else if let Some(value) = value.strip_prefix('#') {
        (16, value)
    } else {
        (10, value)
    };
    let value = i32::from_str_radix(digits, radix).ok()?;

    value.checked_mul(sign)
}

fn android_framework_attr_name(resource_id: u32) -> Option<&'static str> {
    match resource_id {
        // cite: frameworks-base public-final.xml#gravity
        16842927 => Some("gravity"),
        16843093 => Some("height"),
        16843097 => Some("width"),
        16843041 => Some("tint"),
        16843754 => Some("autoMirrored"),
        16843778 => Some("viewportWidth"),
        16843779 => Some("viewportHeight"),
        16843780 => Some("fillColor"),
        16843781 => Some("pathData"),
        16843782 => Some("strokeColor"),
        16843784 => Some("trimPathStart"),
        16843785 => Some("trimPathEnd"),
        16843786 => Some("trimPathOffset"),
        16843979 => Some("strokeAlpha"),
        16843980 => Some("fillAlpha"),
        _ => None,
    }
}

fn typed_array_get_value_arguments(invocation: &DexInvocation) -> Option<(i32, &DexValue)> {
    if invocation.arguments.len() != 3 {
        return None;
    }

    match &invocation.arguments[0] {
        DexValue::Object(_) | DexValue::Reference(_) => {}
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => return None,
    }

    let DexValue::Int(index) = invocation.arguments[1] else {
        return None;
    };

    match &invocation.arguments[2] {
        DexValue::Object(_) | DexValue::Reference(_) => Some((index, &invocation.arguments[2])),
        DexValue::Uninitialized
        | DexValue::Null
        | DexValue::Int(_)
        | DexValue::Long(_)
        | DexValue::Float(_)
        | DexValue::String(_) => None,
    }
}

struct RuntimeTypedValue {
    value_type: i32,
    data: i32,
    string: Option<DexValue>,
}

fn typed_value_from_resource_primitive(
    value: resource_table::ResourcePrimitive,
) -> Option<RuntimeTypedValue> {
    match value {
        resource_table::ResourcePrimitive::String(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_STRING,
            data: 0,
            string: Some(DexValue::String(std::rc::Rc::from(value))),
        }),
        resource_table::ResourcePrimitive::Reference(resource_id) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_REFERENCE,
            data: resource_id as i32,
            string: None,
        }),
        resource_table::ResourcePrimitive::Attribute(resource_id) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_ATTRIBUTE,
            data: resource_id as i32,
            string: None,
        }),
        resource_table::ResourcePrimitive::Float(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_FLOAT,
            data: value as i32,
            string: None,
        }),
        resource_table::ResourcePrimitive::Dimension(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_DIMENSION,
            data: value as i32,
            string: None,
        }),
        resource_table::ResourcePrimitive::Fraction(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_FRACTION,
            data: value as i32,
            string: None,
        }),
        resource_table::ResourcePrimitive::Integer(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_INT_DEC,
            data: value,
            string: None,
        }),
        resource_table::ResourcePrimitive::Hex(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_INT_HEX,
            data: value as i32,
            string: None,
        }),
        resource_table::ResourcePrimitive::Boolean(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_INT_BOOLEAN,
            data: if value { -1 } else { 0 },
            string: None,
        }),
        resource_table::ResourcePrimitive::Color(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_INT_COLOR_ARGB8,
            data: value as i32,
            string: None,
        }),
        resource_table::ResourcePrimitive::Unknown { data_type, data } => Some(RuntimeTypedValue {
            value_type: i32::from(data_type),
            data: data as i32,
            string: None,
        }),
        resource_table::ResourcePrimitive::Array(_) | resource_table::ResourcePrimitive::Bag(_) => {
            None
        }
    }
}

fn android_framework_typed_value_resource(
    resource_id: u32,
) -> Option<resource_table::ResourcePrimitive> {
    android_framework::color_resource(resource_id).map(resource_table::ResourcePrimitive::Color)
}

fn android_framework_color_resource(resource_id: u32) -> Option<resource_table::ResourcePrimitive> {
    android_framework::color_resource(resource_id).map(resource_table::ResourcePrimitive::Color)
}

fn invalid_resource_id_error(resource_kind: &str, resource_id: i32) -> WinedroidError {
    WinedroidError::new(format!(
        "{resource_kind} resource ID must be positive\nresource id: {resource_id}"
    ))
}

fn resource_type_error(resource_kind: &str, resource_id: u32) -> WinedroidError {
    WinedroidError::new(format!(
        "resource 0x{resource_id:08x} is missing or is not a {resource_kind}"
    ))
}

fn unresolved_terminal_reference_error(resource_id: u32, target_id: u32) -> WinedroidError {
    WinedroidError::new(format!(
        "resource 0x{resource_id:08x} retained unresolved reference 0x{target_id:08x}"
    ))
}

fn typed_value_from_xml_value(value: binary_xml::XmlValue) -> Option<RuntimeTypedValue> {
    match value {
        binary_xml::XmlValue::String(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_STRING,
            data: 0,
            string: Some(DexValue::String(std::rc::Rc::from(value))),
        }),
        binary_xml::XmlValue::Reference(resource_id) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_REFERENCE,
            data: resource_id as i32,
            string: None,
        }),
        binary_xml::XmlValue::AttributeReference(resource_id) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_ATTRIBUTE,
            data: resource_id as i32,
            string: None,
        }),
        binary_xml::XmlValue::Float(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_FLOAT,
            data: value as i32,
            string: None,
        }),
        binary_xml::XmlValue::Dimension(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_DIMENSION,
            data: value as i32,
            string: None,
        }),
        binary_xml::XmlValue::Fraction(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_FRACTION,
            data: value as i32,
            string: None,
        }),
        binary_xml::XmlValue::Integer(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_INT_DEC,
            data: value,
            string: None,
        }),
        binary_xml::XmlValue::Hex(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_INT_HEX,
            data: value as i32,
            string: None,
        }),
        binary_xml::XmlValue::Boolean(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_INT_BOOLEAN,
            data: if value { -1 } else { 0 },
            string: None,
        }),
        binary_xml::XmlValue::Color(value) => Some(RuntimeTypedValue {
            value_type: TYPED_VALUE_TYPE_INT_COLOR_ARGB8,
            data: value as i32,
            string: None,
        }),
        binary_xml::XmlValue::Unknown {
            data_type, data, ..
        } => Some(RuntimeTypedValue {
            value_type: i32::from(data_type),
            data: data as i32,
            string: None,
        }),
        binary_xml::XmlValue::Null => None,
    }
}

fn xml_typed_value_resource_id(value: &binary_xml::XmlValue) -> i32 {
    match value {
        binary_xml::XmlValue::Reference(resource_id)
        | binary_xml::XmlValue::AttributeReference(resource_id) => *resource_id as i32,
        binary_xml::XmlValue::Null
        | binary_xml::XmlValue::String(_)
        | binary_xml::XmlValue::Boolean(_)
        | binary_xml::XmlValue::Integer(_)
        | binary_xml::XmlValue::Hex(_)
        | binary_xml::XmlValue::Float(_)
        | binary_xml::XmlValue::Dimension(_)
        | binary_xml::XmlValue::Fraction(_)
        | binary_xml::XmlValue::Color(_)
        | binary_xml::XmlValue::Unknown { .. } => 0,
    }
}

fn typed_value_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/util/TypedValue;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

fn typed_value_char_sequence_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/util/TypedValue;".to_owned(),
        type_descriptor: "Ljava/lang/CharSequence;".to_owned(),
        name: name.to_owned(),
    }
}

impl DexProgram {
    pub(in crate::dex_run) fn context_get_theme(
        &self,
        invocation: Option<&DexInvocation>,
    ) -> Option<DexValue> {
        let context = invocation
            .and_then(|invocation| invocation.arguments.first().cloned())
            .unwrap_or_else(|| self.active_context_value());

        self.ensure_context_theme(&context)
    }

    // cite: frameworks-base ActivityThread.java#performLaunchActivity, getThemeResource
    pub(in crate::dex_run) fn apply_manifest_activity_theme(
        &self,
        class_descriptor: &str,
        receiver: &DexValue,
    ) {
        let theme_resource_id = self
            .activity_themes
            .get(class_descriptor)
            .copied()
            .or(self.application_theme);
        let Some(theme_resource_id) = theme_resource_id else {
            return;
        };
        if theme_resource_id == 0 {
            return;
        }
        if self
            .read_stored_instance_field(receiver, &context_theme_wrapper_theme_resource_field())
            .is_some()
        {
            return;
        }

        self.write_stored_instance_field(
            receiver,
            &context_theme_wrapper_theme_resource_field(),
            DexValue::Int(theme_resource_id as i32),
        );
    }

    pub(in crate::dex_run) fn activity_set_theme(&self, invocation: &DexInvocation) -> Option<()> {
        if invocation.arguments.len() != 2 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }
        let DexValue::Int(style_resource_id) = invocation.arguments[1] else {
            return None;
        };

        let theme = context_theme_reference(&invocation.arguments[0])?;
        let theme_key = map_identity_key(&theme);
        let theme_exists = self.runtime.borrow().themes.contains_key(&theme_key);
        self.runtime.borrow_mut().decor_action_bar = None;
        self.write_stored_instance_field(
            &invocation.arguments[0],
            &context_theme_wrapper_theme_resource_field(),
            DexValue::Int(style_resource_id),
        );
        let theme = self.ensure_context_theme(&invocation.arguments[0])?;
        if theme_exists {
            return self.apply_style_to_theme(&theme, style_resource_id as u32, true);
        }

        Some(())
    }

    pub(in crate::dex_run) fn theme_resolve_attribute(
        &self,
        invocation: &DexInvocation,
    ) -> Result<Option<i32>> {
        if invocation.arguments.len() != 4 {
            return Ok(None);
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return Ok(None);
        }
        let DexValue::Int(attribute_id) = invocation.arguments[1] else {
            return Ok(None);
        };
        if !dex_value_is_object_like(&invocation.arguments[2]) {
            return Ok(None);
        }
        let DexValue::Int(resolve_references) = invocation.arguments[3] else {
            return Ok(None);
        };

        let Some(value) =
            self.resolve_theme_attribute(&invocation.arguments[0], attribute_id as u32)
        else {
            return Ok(Some(0));
        };
        let Some(()) = self.write_resolved_theme_attribute(
            &invocation.arguments[0],
            &invocation.arguments[2],
            attribute_id as u32,
            value,
            resolve_references != 0,
        )?
        else {
            return Ok(None);
        };

        Ok(Some(1))
    }

    pub(in crate::dex_run) fn theme_apply_style(&self, invocation: &DexInvocation) -> Option<()> {
        if invocation.arguments.len() != 3 {
            return None;
        }

        if !dex_value_is_object_like(&invocation.arguments[0]) {
            return None;
        }
        let DexValue::Int(style_resource_id) = invocation.arguments[1] else {
            return None;
        };
        let DexValue::Int(force) = invocation.arguments[2] else {
            return None;
        };

        self.apply_style_to_theme(
            &invocation.arguments[0],
            style_resource_id as u32,
            force != 0,
        )
    }

    pub(in crate::dex_run) fn context_theme_style_snapshot(
        &self,
        context: &DexValue,
    ) -> Vec<DexThemeStyle> {
        let Some(theme) = self.ensure_context_theme(context) else {
            return Vec::new();
        };
        let theme_key = map_identity_key(&theme);

        self.runtime
            .borrow()
            .themes
            .get(&theme_key)
            .map(|theme| {
                theme
                    .styles
                    .iter()
                    .map(|style| DexThemeStyle {
                        resource_id: style.resource_id,
                        force: style.force,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub(in crate::dex_run) fn ensure_context_theme(&self, context: &DexValue) -> Option<DexValue> {
        if !dex_value_is_object_like(context) {
            return None;
        }

        let theme = context_theme_reference(context)?;
        let theme_key = map_identity_key(&theme);
        if self.runtime.borrow().themes.contains_key(&theme_key) {
            return Some(theme);
        }

        let mut theme_state = RuntimeTheme::default();
        if let Some(base_context) =
            self.read_stored_instance_field(context, &context_wrapper_base_field())
        {
            if map_identity_key(&base_context) != map_identity_key(context) {
                if let Some(base_theme) = self.ensure_context_theme(&base_context) {
                    let base_theme_key = map_identity_key(&base_theme);
                    if let Some(base_state) =
                        self.runtime.borrow().themes.get(&base_theme_key).cloned()
                    {
                        theme_state.styles.extend(base_state.styles);
                    }
                }
            }
        }

        if let Some(DexValue::Int(style_resource_id)) =
            self.read_stored_instance_field(context, &context_theme_wrapper_theme_resource_field())
        {
            if style_resource_id > 0 {
                theme_state.styles.push(RuntimeThemeStyle {
                    resource_id: style_resource_id as u32,
                    force: true,
                });
            }
        }

        self.runtime
            .borrow_mut()
            .themes
            .insert(theme_key, theme_state);

        Some(theme)
    }

    fn apply_style_to_theme(
        &self,
        theme: &DexValue,
        style_resource_id: u32,
        force: bool,
    ) -> Option<()> {
        if !dex_value_is_object_like(theme) {
            return None;
        }

        let theme_key = map_identity_key(theme);
        self.runtime
            .borrow_mut()
            .themes
            .entry(theme_key)
            .or_default()
            .styles
            .push(RuntimeThemeStyle {
                resource_id: style_resource_id,
                force,
            });

        Some(())
    }

    pub(in crate::dex_run) fn resolve_theme_attribute(
        &self,
        theme: &DexValue,
        attribute_id: u32,
    ) -> Option<resource_table::ResourcePrimitive> {
        let theme_key = map_identity_key(theme);
        let styles = self
            .runtime
            .borrow()
            .themes
            .get(&theme_key)
            .map(|theme| theme.styles.clone())
            .unwrap_or_default();
        let mut resolved = None;

        for style in styles {
            let value =
                self.resolve_style_attribute(style.resource_id, attribute_id, &mut HashSet::new());
            if let Some(value) = value {
                if style.force || resolved.is_none() {
                    resolved = Some(value);
                }
            }
        }

        resolved
    }

    fn resolve_style_attribute(
        &self,
        style_resource_id: u32,
        attribute_id: u32,
        visited: &mut HashSet<u32>,
    ) -> Option<resource_table::ResourcePrimitive> {
        if !visited.insert(style_resource_id) {
            return None;
        }

        let resource = self
            .resolve_resource_value(style_resource_id)
            .ok()
            .flatten()?;
        let resource_table::ResourcePrimitive::Bag(bag) = resource.value else {
            return None;
        };

        let mut resolved = bag
            .parent
            .and_then(|parent| self.resolve_style_attribute(parent, attribute_id, visited));
        for entry in bag.entries {
            if entry.name == attribute_id {
                resolved = Some(entry.value);
            }
        }

        resolved
    }

    fn write_resolved_theme_attribute(
        &self,
        theme: &DexValue,
        out_value: &DexValue,
        attribute_id: u32,
        value: resource_table::ResourcePrimitive,
        resolve_references: bool,
    ) -> Result<Option<()>> {
        self.write_resolved_theme_attribute_inner(
            theme,
            out_value,
            attribute_id,
            value,
            resolve_references,
            &mut HashSet::new(),
        )
    }

    fn write_resolved_theme_attribute_inner(
        &self,
        theme: &DexValue,
        out_value: &DexValue,
        attribute_id: u32,
        value: resource_table::ResourcePrimitive,
        resolve_references: bool,
        visited: &mut HashSet<u32>,
    ) -> Result<Option<()>> {
        if !visited.insert(attribute_id) {
            return Ok(None);
        }

        let (resource_id, value) = match value {
            resource_table::ResourcePrimitive::Reference(resource_id) if resolve_references => {
                let (resource_id, typed_value) = self
                    .typed_value_from_resource_reference(resource_id)?
                    .ok_or_else(|| resource_type_error("typed value", resource_id))?;
                return Ok(self.write_runtime_typed_value(
                    out_value,
                    resource_id as i32,
                    typed_value,
                ));
            }
            resource_table::ResourcePrimitive::Attribute(next_attribute_id)
                if resolve_references =>
            {
                let Some(value) = self.resolve_theme_attribute(theme, next_attribute_id) else {
                    return Ok(None);
                };
                return self.write_resolved_theme_attribute_inner(
                    theme,
                    out_value,
                    next_attribute_id,
                    value,
                    true,
                    visited,
                );
            }
            value => (attribute_id, value),
        };

        Ok(self.write_typed_value(out_value, resource_id as i32, value))
    }
}

fn context_theme_reference(context: &DexValue) -> Option<DexValue> {
    if !dex_value_is_object_like(context) {
        return None;
    }

    Some(DexValue::Reference(std::rc::Rc::from(format!(
        "Landroid/content/res/Resources$Theme;:{}",
        map_identity_key(context)
    ))))
}

fn context_wrapper_base_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/content/ContextWrapper;".to_owned(),
        type_descriptor: "Landroid/content/Context;".to_owned(),
        name: "mBase".to_owned(),
    }
}

fn context_theme_wrapper_theme_resource_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/view/ContextThemeWrapper;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: "mThemeResource".to_owned(),
    }
}

const CONFIGURATION_FIELD_SIGNATURE_PREFIX: &str = "Landroid/content/res/Configuration;.";
const CONFIG_MCC: i32 = 0x0001;
const CONFIG_MNC: i32 = 0x0002;
const CONFIG_LOCALE: i32 = 0x0004;
const CONFIG_TOUCHSCREEN: i32 = 0x0008;
const CONFIG_KEYBOARD: i32 = 0x0010;
const CONFIG_KEYBOARD_HIDDEN: i32 = 0x0020;
const CONFIG_NAVIGATION: i32 = 0x0040;
const CONFIG_ORIENTATION: i32 = 0x0080;
const CONFIG_SCREEN_LAYOUT: i32 = 0x0100;
const CONFIG_UI_MODE: i32 = 0x0200;
const CONFIG_SCREEN_SIZE: i32 = 0x0400;
const CONFIG_SMALLEST_SCREEN_SIZE: i32 = 0x0800;
const CONFIG_DENSITY: i32 = 0x1000;
const CONFIG_LAYOUT_DIRECTION: i32 = 0x2000;
const CONFIG_COLOR_MODE: i32 = 0x4000;
const CONFIG_FONT_SCALE: i32 = 0x4000_0000;

fn configuration_field_name(field_signature: &str) -> Option<&str> {
    let field = field_signature.strip_prefix(CONFIGURATION_FIELD_SIGNATURE_PREFIX)?;
    let (name, _type_descriptor) = field.split_once(':')?;

    Some(name)
}

fn configuration_change_mask(field_signature: &str) -> i32 {
    match configuration_field_name(field_signature) {
        Some("fontScale") => CONFIG_FONT_SCALE,
        Some("mcc") => CONFIG_MCC,
        Some("mnc") => CONFIG_MNC,
        Some("locale" | "mLocaleList") => CONFIG_LOCALE | CONFIG_LAYOUT_DIRECTION,
        Some("touchscreen") => CONFIG_TOUCHSCREEN,
        Some("keyboard") => CONFIG_KEYBOARD,
        Some("keyboardHidden" | "hardKeyboardHidden") => CONFIG_KEYBOARD_HIDDEN,
        Some("navigation" | "navigationHidden") => CONFIG_NAVIGATION,
        Some("orientation") => CONFIG_ORIENTATION,
        Some("screenLayout") => CONFIG_SCREEN_LAYOUT | CONFIG_LAYOUT_DIRECTION,
        Some("colorMode") => CONFIG_COLOR_MODE,
        Some("uiMode") => CONFIG_UI_MODE,
        Some(
            "screenWidthDp" | "screenHeightDp" | "compatScreenWidthDp" | "compatScreenHeightDp",
        ) => CONFIG_SCREEN_SIZE,
        Some("smallestScreenWidthDp" | "compatSmallestScreenWidthDp") => {
            CONFIG_SMALLEST_SCREEN_SIZE
        }
        Some("densityDpi") => CONFIG_DENSITY,
        Some("seq" | "assetsSeq" | "windowConfiguration") => 0,
        Some(_) | None => 0,
    }
}

fn configuration_font_scale_field() -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/content/res/Configuration;".to_owned(),
        type_descriptor: "F".to_owned(),
        name: "fontScale".to_owned(),
    }
}

fn configuration_ui_mode_field() -> DexFieldId {
    configuration_int_field("uiMode")
}

fn configuration_int_field(name: &str) -> DexFieldId {
    DexFieldId {
        class_descriptor: "Landroid/content/res/Configuration;".to_owned(),
        type_descriptor: "I".to_owned(),
        name: name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_RES_STRING_POOL_TYPE: u16 = 0x0001;
    const TEST_RES_TABLE_TYPE: u16 = 0x0002;
    const TEST_RES_TABLE_PACKAGE_TYPE: u16 = 0x0200;
    const TEST_RES_TABLE_TYPE_TYPE: u16 = 0x0201;
    const TEST_UTF8_FLAG: u32 = 0x0000_0100;
    const TEST_ENTRY_FLAG_COMPLEX: u16 = 0x0001;
    const TEST_VALUE_TYPE_REFERENCE: u8 = 0x01;
    const TEST_VALUE_TYPE_ATTRIBUTE: u8 = 0x02;
    const TEST_VALUE_TYPE_STRING: u8 = 0x03;
    const TEST_VALUE_TYPE_FLOAT: u8 = 0x04;
    const TEST_VALUE_TYPE_DIMENSION: u8 = 0x05;
    const TEST_VALUE_TYPE_FRACTION: u8 = 0x06;
    const TEST_VALUE_TYPE_INTEGER: u8 = 0x10;
    const TEST_VALUE_TYPE_HEX: u8 = 0x11;
    const TEST_VALUE_TYPE_BOOLEAN: u8 = 0x12;
    const TEST_VALUE_TYPE_COLOR: u8 = 0x1c;

    enum TestResourceEntry {
        Simple { value_type: u8, data: u32 },
        Array(Vec<(u8, u32)>),
    }

    struct TestResourceChunk {
        config: Vec<u8>,
        entries: Vec<Option<TestResourceEntry>>,
    }

    #[test]
    fn fresh_xml_resource_parser_reference_returns_distinct_instances() {
        let program = DexProgram::new(Vec::new());
        let first = program.fresh_xml_resource_parser_reference(2131624225);
        let second = program.fresh_xml_resource_parser_reference(2131624225);

        assert_ne!(first, second);
        assert_eq!(
            layout_id_from_xml_parser_reference(&first),
            Some(2131624225)
        );
        assert_eq!(
            layout_id_from_xml_parser_reference(&second),
            Some(2131624225)
        );
    }

    // cite: frameworks-base DisplayContent.java#computeScreenConfiguration
    // cite: frameworks-base Configuration.java#isScreenRound, UI_MODE_TYPE_WATCH
    #[test]
    fn watch_configuration_reports_round_screen_and_watch_ui_mode() {
        let program = DexProgram::with_config(Vec::new(), WinedroidConfig::watch());
        let configuration = DexValue::Reference(std::rc::Rc::from(
            "Landroid/content/res/Configuration;.bootstrap",
        ));
        let invocation = DexInvocation {
            kind: crate::dex_interpreter::DexInvokeKind::Virtual,
            range: false,
            method_index: 0,
            argument_registers: Default::default(),
            arguments: vec![configuration.clone()].into_iter().collect(),
        };

        assert_eq!(
            program.read_stored_instance_field(&configuration, &configuration_int_field("uiMode")),
            Some(DexValue::Int(0x16))
        );
        assert_eq!(
            program.read_stored_instance_field(
                &configuration,
                &configuration_int_field("screenLayout")
            ),
            Some(DexValue::Int(0x251))
        );
        assert_eq!(program.configuration_is_screen_round(&invocation), Some(1));

        let resource_config = program.resource_resolution_config();
        assert_eq!(resource_config.ui_mode, 0x16);
        assert_eq!(resource_config.screen_layout2, 0x02);

        let phone = DexProgram::with_config(Vec::new(), WinedroidConfig::default());
        assert_eq!(phone.configuration_is_screen_round(&invocation), Some(0));
    }

    // cite: frameworks-base ResourcesManager.java#applyDisplayMetricsToConfiguration
    // cite: frameworks-base Configuration.java#reduceScreenLayout, setLocales, setLayoutDirection
    #[test]
    fn configuration_and_resource_resolution_share_non_default_display_metrics() {
        let config = WinedroidConfig::with_display_size(1000, 600);
        let program = DexProgram::with_config(Vec::new(), config);
        let configuration = DexValue::Reference(std::rc::Rc::from(
            "Landroid/content/res/Configuration;.bootstrap",
        ));

        for (name, expected) in [
            ("touchscreen", CONFIGURATION_TOUCHSCREEN_NOTOUCH),
            ("densityDpi", BOOTSTRAP_DISPLAY_DENSITY_DPI),
            ("screenWidthDp", 1000),
            ("screenHeightDp", 600),
            ("smallestScreenWidthDp", 600),
            ("compatScreenWidthDp", 1000),
            ("compatScreenHeightDp", 600),
            ("compatSmallestScreenWidthDp", 600),
            ("orientation", CONFIGURATION_ORIENTATION_LANDSCAPE),
            ("screenLayout", 0x1000_0163),
        ] {
            let field = configuration_int_field(name);
            let actual = program.read_stored_instance_field(&configuration, &field);

            assert_eq!(actual, Some(DexValue::Int(expected)));
        }

        let resource_config = program.resource_resolution_config();
        assert_eq!(
            resource_config.orientation,
            CONFIGURATION_ORIENTATION_LANDSCAPE as u8
        );
        assert_eq!(resource_config.smallest_screen_width_dp, 600);
        assert_eq!(resource_config.screen_width_dp, 1000);
        assert_eq!(resource_config.screen_height_dp, 600);
        assert_eq!(resource_config.screen_width_px, 1000);
        assert_eq!(resource_config.screen_height_px, 600);
        assert_eq!(resource_config.screen_layout, 0x63);
    }

    // cite: frameworks-base AssetManager2.cpp#AssetManager2::ResolveReference
    // cite: frameworks-base AssetManager.java#getResourceText
    #[test]
    fn configured_string_resolution_accepts_twenty_and_preserves_the_twenty_first_reference() {
        let twenty = test_string_reference_chain(20);
        let twenty_one = test_string_reference_chain(21);

        assert_eq!(
            twenty
                .resolve_string_resource_text(0x7f01_0000)
                .expect("twenty references should resolve"),
            Some("terminal".to_owned())
        );
        assert_eq!(
            twenty_one
                .resolve_string_resource_text(0x7f01_0000)
                .expect("twenty-one references should preserve the last reference"),
            Some(format!("@{}", 0x7f01_0015_u32))
        );
    }

    // cite: frameworks-base AssetManager2.cpp#AssetManager2::ResolveReference
    // cite: frameworks-base AssetManager.java#getResourceText
    #[test]
    fn configured_string_resolution_preserves_cycles_and_reports_missing_targets() {
        let keys = vec!["first".to_owned(), "second".to_owned()];
        let cycle = test_resource_table(
            "string",
            &keys,
            &["unused".to_owned()],
            vec![TestResourceChunk {
                config: Vec::new(),
                entries: vec![
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_REFERENCE,
                        data: 0x7f01_0001,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_REFERENCE,
                        data: 0x7f01_0000,
                    }),
                ],
            }],
        );
        let missing = test_resource_table(
            "string",
            &["missing".to_owned()],
            &["unused".to_owned()],
            vec![TestResourceChunk {
                config: Vec::new(),
                entries: vec![Some(TestResourceEntry::Simple {
                    value_type: TEST_VALUE_TYPE_REFERENCE,
                    data: 0x7f01_ffff,
                })],
            }],
        );
        let cycle_program = test_program_with_resources(cycle);
        let missing_program = test_program_with_resources(missing);

        let cycle_value = cycle_program
            .resolve_string_resource_text(0x7f01_0000)
            .expect("reference cycle should preserve the last reference");
        let missing_error = missing_program
            .resolve_string_resource_text(0x7f01_0000)
            .expect_err("missing reference target should fail");

        assert_eq!(cycle_value, Some(format!("@{}", 0x7f01_0001_u32)));
        assert!(missing_error
            .to_string()
            .contains("references missing resource"));
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable_config::match, ResTable_config::isBetterThan, ResTable::resolveReference
    #[test]
    fn configured_string_alias_selects_terminal_for_the_same_display_configuration() {
        let keys = vec!["alias".to_owned(), "terminal".to_owned()];
        let resources = test_resource_table(
            "string",
            &keys,
            &["default".to_owned(), "landscape".to_owned()],
            vec![
                TestResourceChunk {
                    config: Vec::new(),
                    entries: vec![
                        Some(TestResourceEntry::Simple {
                            value_type: TEST_VALUE_TYPE_REFERENCE,
                            data: 0x7f01_0001,
                        }),
                        Some(TestResourceEntry::Simple {
                            value_type: TEST_VALUE_TYPE_STRING,
                            data: 0,
                        }),
                    ],
                },
                TestResourceChunk {
                    config: test_display_resource_config(2, 600, 1000, 600),
                    entries: vec![
                        None,
                        Some(TestResourceEntry::Simple {
                            value_type: TEST_VALUE_TYPE_STRING,
                            data: 1,
                        }),
                    ],
                },
            ],
        );
        let landscape = DexProgram::with_resource_context_and_config(
            Vec::new(),
            None,
            None,
            None,
            None,
            Some(resources.clone()),
            WinedroidConfig::with_display_size(1000, 600),
        );
        let portrait = DexProgram::with_resource_context_and_config(
            Vec::new(),
            None,
            None,
            None,
            None,
            Some(resources),
            WinedroidConfig::with_display_size(600, 1000),
        );

        assert_eq!(
            landscape
                .resolve_string_resource_text(0x7f01_0000)
                .expect("landscape alias should resolve"),
            Some("landscape".to_owned())
        );
        assert_eq!(
            portrait
                .resolve_string_resource_text(0x7f01_0000)
                .expect("portrait alias should resolve"),
            Some("default".to_owned())
        );
    }

    // cite: frameworks-base Resources.java#getValue
    // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
    // cite: frameworks-base public-final.xml#white
    // cite: frameworks-base colors.xml#white
    #[test]
    fn typed_value_reference_flag_and_color_resolution_preserve_framework_terminal() {
        let framework_white = 0x0106_000b;
        let resources = test_resource_table(
            "color",
            &["framework_white".to_owned()],
            &[],
            vec![TestResourceChunk {
                config: Vec::new(),
                entries: vec![Some(TestResourceEntry::Simple {
                    value_type: TEST_VALUE_TYPE_REFERENCE,
                    data: framework_white,
                })],
            }],
        );
        let program = test_program_with_resources(resources);

        assert_eq!(
            program
                .resolve_typed_value_resource(0x7f01_0000, false)
                .expect("unresolved typed value should parse"),
            Some(resource_table::ResourcePrimitive::Reference(
                framework_white
            ))
        );
        assert_eq!(
            program
                .resolve_typed_value_resource(0x7f01_0000, true)
                .expect("resolved typed value should parse"),
            Some(resource_table::ResourcePrimitive::Color(0xffff_ffff))
        );
        assert_eq!(
            program
                .resolve_color_resource(0x7f01_0000)
                .expect("framework color alias should parse"),
            Some(ResolvedColorResource::Integer(0xffff_ffff))
        );
    }

    // cite: frameworks-base Resources.java#getBoolean, getColor, getDimension, getDimensionPixelOffset, getDimensionPixelSize, getText
    // cite: frameworks-base TypedValue.java#coerceToString, complexToDimensionPixelOffset, complexToDimensionPixelSize
    // cite: libcore Float.java#toString
    #[test]
    fn resources_scalar_type_rules_and_typed_value_string_coercion_match_android() {
        let keys = (0..14)
            .map(|index| format!("entry_{index}"))
            .collect::<Vec<_>>();
        let resources = test_resource_table(
            "value",
            &keys,
            &["true".to_owned()],
            vec![TestResourceChunk {
                config: Vec::new(),
                entries: vec![
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_DIMENSION,
                        data: 0x00c0_0020,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_FLOAT,
                        data: 1.5_f32.to_bits(),
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_STRING,
                        data: 0,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_COLOR,
                        data: 0xff00_ff00,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_BOOLEAN,
                        data: u32::MAX,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: 0x13,
                        data: 42,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_ATTRIBUTE,
                        data: 42,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_FRACTION,
                        data: 0x4000_0030,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_INTEGER,
                        data: (-42_i32) as u32,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_HEX,
                        data: 0xab,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_REFERENCE,
                        data: 0,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_FLOAT,
                        data: f32::NAN.to_bits(),
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_FLOAT,
                        data: f32::INFINITY.to_bits(),
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_FLOAT,
                        data: f32::NEG_INFINITY.to_bits(),
                    }),
                ],
            }],
        );
        let program = test_program_with_resources(resources);
        let dimension = test_resources_invocation(0x7f01_0000);
        let float = test_resources_invocation(0x7f01_0001);
        let string = test_resources_invocation(0x7f01_0002);
        let color = test_resources_invocation(0x7f01_0003);
        let boolean = test_resources_invocation(0x7f01_0004);
        let reserved_integer = test_resources_invocation(0x7f01_0005);

        assert_eq!(
            program
                .resources_get_dimension(&dimension)
                .expect("dimension should resolve"),
            Some(1.5)
        );
        assert_eq!(
            program
                .resources_get_dimension_pixel_offset(&dimension)
                .expect("dimension offset should resolve"),
            Some(1)
        );
        assert_eq!(
            program
                .resources_get_dimension_pixel_size(&dimension)
                .expect("dimension size should resolve"),
            Some(2)
        );
        assert!(program.resources_get_dimension(&float).is_err());
        assert!(program.resources_get_boolean(&string).is_err());
        assert_eq!(
            program
                .resources_get_boolean(&color)
                .expect("color is in the integer type family"),
            Some(1)
        );
        assert_eq!(
            program
                .resources_get_boolean(&reserved_integer)
                .expect("reserved integer type should resolve"),
            Some(1)
        );
        assert_eq!(
            program
                .resources_get_color(&boolean)
                .expect("boolean is in the integer type family"),
            Some(DexExecutionResult::Int(-1))
        );
        assert_eq!(
            program
                .resources_get_color(&reserved_integer)
                .expect("reserved integer color should resolve"),
            Some(DexExecutionResult::Int(42))
        );
        assert_eq!(
            program
                .resources_get_color(&string)
                .expect("string color requires the complex-color loader"),
            None
        );

        for (index, expected) in [
            (0, "1.5px"),
            (1, "1.5"),
            (2, "true"),
            (3, "#ff00ff00"),
            (4, "true"),
            (5, "42"),
            (6, "?42"),
            (7, "50.0%"),
            (8, "-42"),
            (9, "0xab"),
            (10, "@0"),
            (11, "NaN"),
            (12, "Infinity"),
            (13, "-Infinity"),
        ] {
            assert_eq!(
                program
                    .resolve_string_resource_text(0x7f01_0000 + index)
                    .expect("typed value should coerce"),
                Some(expected.to_owned())
            );
        }
    }

    // cite: frameworks-base AssetManager.java#getResourceValue
    // cite: frameworks-base AssetManager2.cpp#AssetManager2::GetResource, AssetManager2::ResolveReference
    // cite: frameworks-base ResourcesImpl.java#getValue
    // cite: frameworks-base Resources.java#getValue
    #[test]
    fn resources_get_value_rejects_direct_bags_and_tracks_resolved_resource_ids() {
        let keys = (0..5)
            .map(|index| format!("entry_{index}"))
            .collect::<Vec<_>>();
        let resources = test_resource_table(
            "array",
            &keys,
            &["item".to_owned()],
            vec![TestResourceChunk {
                config: Vec::new(),
                entries: vec![
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_REFERENCE,
                        data: 0x7f01_0001,
                    }),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_COLOR,
                        data: 0xff12_3456,
                    }),
                    Some(TestResourceEntry::Array(vec![(TEST_VALUE_TYPE_STRING, 0)])),
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_REFERENCE,
                        data: 0x7f01_0004,
                    }),
                    Some(TestResourceEntry::Array(vec![(TEST_VALUE_TYPE_STRING, 0)])),
                ],
            }],
        );
        let program = test_program_with_resources(resources);
        let unresolved = program.allocate_object("Landroid/util/TypedValue;".to_owned());
        let resolved = program.allocate_object("Landroid/util/TypedValue;".to_owned());
        let bag_alias = program.allocate_object("Landroid/util/TypedValue;".to_owned());

        assert_eq!(
            program
                .resources_get_value(&test_get_value_invocation(
                    0x7f01_0000,
                    unresolved.clone(),
                    false,
                ))
                .expect("unresolved alias should be returned"),
            Some(())
        );
        assert_runtime_typed_value(
            &program,
            &unresolved,
            TYPED_VALUE_TYPE_REFERENCE,
            0x7f01_0001,
            0x7f01_0000,
        );

        assert_eq!(
            program
                .resources_get_value(&test_get_value_invocation(
                    0x7f01_0000,
                    resolved.clone(),
                    true,
                ))
                .expect("resolved alias should be returned"),
            Some(())
        );
        assert_runtime_typed_value(
            &program,
            &resolved,
            TYPED_VALUE_TYPE_INT_COLOR_ARGB8,
            0xff12_3456_u32 as i32,
            0x7f01_0001,
        );

        assert!(program
            .resources_get_value(&test_get_value_invocation(
                0x7f01_0002,
                program.allocate_object("Landroid/util/TypedValue;".to_owned()),
                true,
            ))
            .is_err());
        assert_eq!(
            program
                .resources_get_value(&test_get_value_invocation(
                    0x7f01_0003,
                    bag_alias.clone(),
                    true,
                ))
                .expect("alias to bag should remain a reference"),
            Some(())
        );
        assert_runtime_typed_value(
            &program,
            &bag_alias,
            TYPED_VALUE_TYPE_REFERENCE,
            0x7f01_0004,
            0x7f01_0004,
        );

        let depth_program = test_string_reference_chain(21);
        let depth_value = depth_program.allocate_object("Landroid/util/TypedValue;".to_owned());
        assert_eq!(
            depth_program
                .resources_get_value(&test_get_value_invocation(
                    0x7f01_0000,
                    depth_value.clone(),
                    true,
                ))
                .expect("reference limit should preserve the last value"),
            Some(())
        );
        assert_runtime_typed_value(
            &depth_program,
            &depth_value,
            TYPED_VALUE_TYPE_REFERENCE,
            0x7f01_0015,
            0x7f01_0014,
        );
    }

    // cite: frameworks-base AttributeResolution.cpp#ResolveAttrs, ApplyStyle, RetrieveAttributes
    // cite: frameworks-base AssetManager2.cpp#AssetManager2::GetResource, AssetManager2::ResolveReference
    // cite: frameworks-base TypedArray.java#getString, getText, getValue
    #[test]
    fn typed_array_reference_to_bag_remains_a_reference() {
        let resources = test_resource_table(
            "array",
            &["alias".to_owned(), "bag".to_owned()],
            &["item".to_owned()],
            vec![TestResourceChunk {
                config: Vec::new(),
                entries: vec![
                    Some(TestResourceEntry::Simple {
                        value_type: TEST_VALUE_TYPE_REFERENCE,
                        data: 0x7f01_0001,
                    }),
                    Some(TestResourceEntry::Array(vec![(TEST_VALUE_TYPE_STRING, 0)])),
                ],
            }],
        );
        let program = test_program_with_resources(resources);
        let resource = program
            .resolve_typed_array_resource_reference(0x7f01_0000)
            .expect("bag reference should resolve");

        assert_eq!(resource.resource_id, 0x7f01_0001);
        assert_eq!(
            resource.value,
            resource_table::ResourcePrimitive::Reference(0x7f01_0001)
        );
        assert_eq!(
            program
                .typed_array_resource_primitive_text(resource_table::ResourcePrimitive::Reference(
                    0x7f01_0000
                ),)
                .expect("bag reference should coerce"),
            Some(format!("@{}", 0x7f01_0001_u32))
        );
    }

    // cite: frameworks-base ResourceTypes.cpp#ResTable::resolveReference
    // cite: frameworks-base AssetManager.java#getResourceTextArray
    #[test]
    fn text_array_alias_and_each_element_receive_independent_reference_budgets() {
        let mut entries = Vec::new();
        for index in 0..20_u32 {
            entries.push(Some(TestResourceEntry::Simple {
                value_type: TEST_VALUE_TYPE_REFERENCE,
                data: 0x7f01_0000 + index + 1,
            }));
        }
        entries.push(Some(TestResourceEntry::Array(vec![
            (TEST_VALUE_TYPE_REFERENCE, 0x7f01_0015),
            (TEST_VALUE_TYPE_REFERENCE, 0x7f01_002a),
            (TEST_VALUE_TYPE_REFERENCE, 0),
            (TEST_VALUE_TYPE_INTEGER, 42),
            (TEST_VALUE_TYPE_BOOLEAN, u32::MAX),
        ])));
        for index in 21..41_u32 {
            entries.push(Some(TestResourceEntry::Simple {
                value_type: TEST_VALUE_TYPE_REFERENCE,
                data: 0x7f01_0000 + index + 1,
            }));
        }
        entries.push(Some(TestResourceEntry::Simple {
            value_type: TEST_VALUE_TYPE_STRING,
            data: 0,
        }));
        for index in 42..62_u32 {
            entries.push(Some(TestResourceEntry::Simple {
                value_type: TEST_VALUE_TYPE_REFERENCE,
                data: 0x7f01_0000 + index + 1,
            }));
        }
        entries.push(Some(TestResourceEntry::Simple {
            value_type: TEST_VALUE_TYPE_STRING,
            data: 1,
        }));
        let keys = (0..entries.len())
            .map(|index| format!("entry_{index}"))
            .collect::<Vec<_>>();
        let resources = test_resource_table(
            "array",
            &keys,
            &["first".to_owned(), "second".to_owned()],
            vec![TestResourceChunk {
                config: Vec::new(),
                entries,
            }],
        );
        let program = test_program_with_resources(resources);

        assert_eq!(
            program
                .resolve_text_array_resource(0x7f01_0000)
                .expect("text array references should resolve"),
            Some(vec![
                Some("first".to_owned()),
                Some("second".to_owned()),
                None,
                None,
                None,
            ])
        );
    }

    fn test_string_reference_chain(reference_count: usize) -> DexProgram {
        let mut entries = Vec::with_capacity(reference_count + 1);
        for index in 0..reference_count {
            entries.push(Some(TestResourceEntry::Simple {
                value_type: TEST_VALUE_TYPE_REFERENCE,
                data: 0x7f01_0000 + index as u32 + 1,
            }));
        }
        entries.push(Some(TestResourceEntry::Simple {
            value_type: TEST_VALUE_TYPE_STRING,
            data: 0,
        }));
        let keys = (0..entries.len())
            .map(|index| format!("entry_{index}"))
            .collect::<Vec<_>>();
        let resources = test_resource_table(
            "string",
            &keys,
            &["terminal".to_owned()],
            vec![TestResourceChunk {
                config: Vec::new(),
                entries,
            }],
        );

        test_program_with_resources(resources)
    }

    fn test_resources_invocation(resource_id: i32) -> DexInvocation {
        DexInvocation {
            kind: crate::dex_interpreter::DexInvokeKind::Virtual,
            range: false,
            method_index: 0,
            argument_registers: Default::default(),
            arguments: smallvec::smallvec![
                DexValue::Reference(std::rc::Rc::from("resources")),
                DexValue::Int(resource_id),
            ],
        }
    }

    fn test_get_value_invocation(
        resource_id: i32,
        out_value: DexValue,
        resolve_references: bool,
    ) -> DexInvocation {
        DexInvocation {
            kind: crate::dex_interpreter::DexInvokeKind::Virtual,
            range: false,
            method_index: 0,
            argument_registers: Default::default(),
            arguments: smallvec::smallvec![
                DexValue::Reference(std::rc::Rc::from("resources")),
                DexValue::Int(resource_id),
                out_value,
                DexValue::Int(i32::from(resolve_references)),
            ],
        }
    }

    fn assert_runtime_typed_value(
        program: &DexProgram,
        value: &DexValue,
        value_type: i32,
        data: i32,
        resource_id: i32,
    ) {
        assert_eq!(
            program.read_stored_instance_field(value, &typed_value_int_field("type")),
            Some(DexValue::Int(value_type))
        );
        assert_eq!(
            program.read_stored_instance_field(value, &typed_value_int_field("data")),
            Some(DexValue::Int(data))
        );
        assert_eq!(
            program.read_stored_instance_field(value, &typed_value_int_field("resourceId")),
            Some(DexValue::Int(resource_id))
        );
    }

    fn test_program_with_resources(resources: Vec<u8>) -> DexProgram {
        DexProgram::with_resource_context(Vec::new(), None, None, None, None, Some(resources))
    }

    fn test_resource_table(
        type_name: &str,
        key_names: &[String],
        global_strings: &[String],
        chunks: Vec<TestResourceChunk>,
    ) -> Vec<u8> {
        let global_string_pool = test_string_pool(global_strings);
        let type_string_pool = test_string_pool(&[type_name.to_owned()]);
        let key_string_pool = test_string_pool(key_names);
        let type_chunks = chunks
            .into_iter()
            .map(|chunk| test_type_chunk(key_names.len(), chunk))
            .collect::<Vec<_>>();
        let type_strings_offset = 288_u32;
        let key_strings_offset = type_strings_offset + type_string_pool.len() as u32;
        let type_chunks_size = type_chunks.iter().map(Vec::len).sum::<usize>();
        let package_size = 288 + type_string_pool.len() + key_string_pool.len() + type_chunks_size;
        let mut package = Vec::new();

        test_push_u16(&mut package, TEST_RES_TABLE_PACKAGE_TYPE);
        test_push_u16(&mut package, 288);
        test_push_u32(&mut package, package_size as u32);
        test_push_u32(&mut package, 0x7f);
        package.extend(vec![0; 256]);
        test_push_u32(&mut package, type_strings_offset);
        test_push_u32(&mut package, 0);
        test_push_u32(&mut package, key_strings_offset);
        test_push_u32(&mut package, 0);
        test_push_u32(&mut package, 0);
        package.extend(type_string_pool);
        package.extend(key_string_pool);
        for chunk in type_chunks {
            package.extend(chunk);
        }

        let table_size = 12 + global_string_pool.len() + package.len();
        let mut table = Vec::new();
        test_push_u16(&mut table, TEST_RES_TABLE_TYPE);
        test_push_u16(&mut table, 12);
        test_push_u32(&mut table, table_size as u32);
        test_push_u32(&mut table, 1);
        table.extend(global_string_pool);
        table.extend(package);
        table
    }

    fn test_type_chunk(entry_count: usize, chunk: TestResourceChunk) -> Vec<u8> {
        assert_eq!(chunk.entries.len(), entry_count);
        let mut offsets = Vec::with_capacity(entry_count);
        let mut entry_data = Vec::new();

        for (key_index, entry) in chunk.entries.into_iter().enumerate() {
            let Some(entry) = entry else {
                offsets.push(u32::MAX);
                continue;
            };
            offsets.push(entry_data.len() as u32);
            match entry {
                TestResourceEntry::Simple { value_type, data } => {
                    test_push_u16(&mut entry_data, 8);
                    test_push_u16(&mut entry_data, 0);
                    test_push_u32(&mut entry_data, key_index as u32);
                    test_push_u16(&mut entry_data, 8);
                    entry_data.push(0);
                    entry_data.push(value_type);
                    test_push_u32(&mut entry_data, data);
                }
                TestResourceEntry::Array(values) => {
                    test_push_u16(&mut entry_data, 16);
                    test_push_u16(&mut entry_data, TEST_ENTRY_FLAG_COMPLEX);
                    test_push_u32(&mut entry_data, key_index as u32);
                    test_push_u32(&mut entry_data, 0);
                    test_push_u32(&mut entry_data, values.len() as u32);
                    for (index, (value_type, data)) in values.into_iter().enumerate() {
                        test_push_u32(&mut entry_data, index as u32);
                        test_push_u16(&mut entry_data, 8);
                        entry_data.push(0);
                        entry_data.push(value_type);
                        test_push_u32(&mut entry_data, data);
                    }
                }
            }
        }

        let header_size = 20 + chunk.config.len();
        let entries_start = header_size + offsets.len() * 4;
        let chunk_size = entries_start + entry_data.len();
        let mut data = Vec::new();
        test_push_u16(&mut data, TEST_RES_TABLE_TYPE_TYPE);
        test_push_u16(&mut data, header_size as u16);
        test_push_u32(&mut data, chunk_size as u32);
        data.push(1);
        data.push(0);
        test_push_u16(&mut data, 0);
        test_push_u32(&mut data, entry_count as u32);
        test_push_u32(&mut data, entries_start as u32);
        data.extend(chunk.config);
        for offset in offsets {
            test_push_u32(&mut data, offset);
        }
        data.extend(entry_data);
        data
    }

    fn test_display_resource_config(
        orientation: u8,
        smallest_width_dp: u16,
        width_dp: u16,
        height_dp: u16,
    ) -> Vec<u8> {
        let mut config = vec![0; 36];
        test_write_u32(&mut config, 0, 36);
        config[12] = orientation;
        test_write_u16(&mut config, 30, smallest_width_dp);
        test_write_u16(&mut config, 32, width_dp);
        test_write_u16(&mut config, 34, height_dp);
        config
    }

    fn test_string_pool(values: &[String]) -> Vec<u8> {
        let mut string_data = Vec::new();
        let mut offsets = Vec::with_capacity(values.len());
        for value in values {
            offsets.push(string_data.len() as u32);
            assert!(value.len() < 0x80);
            string_data.push(value.len() as u8);
            string_data.push(value.len() as u8);
            string_data.extend(value.as_bytes());
            string_data.push(0);
        }

        let header_size = 28;
        let strings_start = header_size + offsets.len() * 4;
        let chunk_size = strings_start + string_data.len();
        let mut pool = Vec::new();
        test_push_u16(&mut pool, TEST_RES_STRING_POOL_TYPE);
        test_push_u16(&mut pool, header_size as u16);
        test_push_u32(&mut pool, chunk_size as u32);
        test_push_u32(&mut pool, values.len() as u32);
        test_push_u32(&mut pool, 0);
        test_push_u32(&mut pool, TEST_UTF8_FLAG);
        test_push_u32(&mut pool, strings_start as u32);
        test_push_u32(&mut pool, 0);
        for offset in offsets {
            test_push_u32(&mut pool, offset);
        }
        pool.extend(string_data);
        pool
    }

    fn test_push_u16(data: &mut Vec<u8>, value: u16) {
        data.extend(value.to_le_bytes());
    }

    fn test_push_u32(data: &mut Vec<u8>, value: u32) {
        data.extend(value.to_le_bytes());
    }

    fn test_write_u16(data: &mut [u8], offset: usize, value: u16) {
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn test_write_u32(data: &mut [u8], offset: usize, value: u32) {
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn layout_id_from_xml_parser_reference(value: &DexValue) -> Option<i32> {
        let DexValue::Reference(reference) = value else {
            return None;
        };
        let marker = ":layout/";
        let start = reference.find(marker)? + marker.len();
        let digits = reference[start..]
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect::<String>();

        digits.parse().ok()
    }
}
