# Comprehensive Analysis: Rust Enum Representations in JSONSchema

## Overview

This document provides an exhaustive analysis of all the different ways Rust enums in the `komorebi-for-mac` project are represented in the generated schema files (`schema.json` and `schema.bar.json`). The project uses `schemars` with the `JsonSchema` derive macro for schema generation.

---

## Pattern 1: Simple Unit Enums (String Constants)

**Rust Pattern:** Enums where all variants have no associated data.

**Serde Attributes:** None (default) or `#[serde(rename_all = "...")]`

**JSONSchema Representation:** Uses `oneOf` with each variant as `{ "type": "string", "const": "VariantName" }`

**Source Example:** `ApplicationIdentifier` in `komorebi/src/core/mod.rs`
```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum ApplicationIdentifier {
    /// Executable name
    Exe,
    /// Title
    Title,
    /// Path
    Path,
    /// App Bundle name
    Name,
}
```

**Schema Output:**
```json
{
  "description": "Application identifier",
  "oneOf": [
    { "description": "Executable name", "type": "string", "const": "Exe" },
    { "description": "Title", "type": "string", "const": "Title" },
    { "description": "Path", "type": "string", "const": "Path" },
    { "description": "App Bundle name", "type": "string", "const": "Name" }
  ]
}
```

**Title/Discriminant Location:**
| Field | JSON Path | Description |
|-------|-----------|-------------|
| Variant Name | `oneOf[n].const` | The variant name as a string constant |
| Variant Description | `oneOf[n].description` | Doc comment from the variant |
| Enum Description | `description` | Doc comment from the enum |

**Examples in schema.json:**
| Definition | Lines |
|------------|-------|
| `ApplicationIdentifier` | L570-594 |
| `Axis` | L622-641 |
| `CrossBoundaryBehaviour` | L2410-2424 |
| `FloatingLayerBehaviour` | L2476-2490 |
| `MatchingStrategy` | L2898-2952 |
| `MoveBehaviour` | L3050-3069 |
| `OperationBehaviour` | L3070-3084 |
| `Placement` | L3141-3160 |
| `PredefinedAspectRatio` | L3161-3180 |
| `ThemeVariant` | L3366-3380 |
| `WindowContainerBehaviour` | L3413-3427 |
| `WindowHidingPosition` | L3428-3442 |
| `Catppuccin` | L2237-2261 |
| `CatppuccinValue` | L2262-2396 |
| `Base16Value` | L2080-2164 |
| `Base16` | L642-1991 (270 theme variants) |

**Examples in schema.bar.json:**
| Definition | Lines |
|------------|-------|
| `ApplicationsDisplayFormat` | L268-287 |
| `DisplayFormat` | L2241-2270 |
| `GroupingStyle` | L2441-2480 |
| `LabelPrefix` | L2958-2982 |

---

## Pattern 2: Unit Enum with Default Value

**Rust Pattern:** Unit enum with `#[schemars(extend("default" = Self::Variant))]`

**JSONSchema Representation:** Same as Pattern 1, but with an additional `"default"` field at the enum level.

**Source Example:** `DefaultLayout` in `komorebi/src/core/default_layout.rs`
```rust
#[derive(Clone, Copy, Debug, Serialize, Deserialize, Display, EnumString, ValueEnum, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schemars", schemars(extend("default" = Self::BSP)))]
pub enum DefaultLayout {
    /// BSP Layout
    BSP,
    /// Columns Layout
    Columns,
    // ... more variants
}
```

**Schema Output:**
```json
{
  "description": "A predefined komorebi layout",
  "default": "BSP",
  "oneOf": [
    { "description": "BSP Layout", "type": "string", "const": "BSP" },
    { "description": "Columns Layout", "type": "string", "const": "Columns" },
    ...
  ]
}
```

**Title/Discriminant Location:**
| Field | JSON Path | Description |
|-------|-----------|-------------|
| Variant Name | `oneOf[n].const` | The variant name as a string constant |
| Default Value | `default` | The default variant name (at enum root level) |
| Variant Description | `oneOf[n].description` | Doc comment from the variant |

**Examples:**
| Schema | Definition | Lines |
|--------|------------|-------|
| schema.json | `DefaultLayout` | L2425-2475 |
| schema.bar.json | `DefaultLayout` | L2190-2240 |

---

## Pattern 3: Mixed Enum (Unit Variants + Newtype Variants)

**Rust Pattern:** Enum with mostly unit variants plus one or more tuple/newtype variants containing data. Uses default external tagging.

**JSONSchema Representation:** Uses `oneOf` where:
- Unit variants become `{ "type": "string", "const": "Name" }`
- Newtype variants become objects with the variant name as a property key

**Source Example:** `DateFormat` in `komorebi-bar/src/widgets/date.rs`
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum DateFormat {
    /// Month/Date/Year format (09/08/24)
    MonthDateYear,
    /// Year-Month-Date format (2024-09-08)
    YearMonthDate,
    /// Custom format
    #[cfg_attr(feature = "schemars", schemars(title = "Custom"))]
    Custom(String),
    /// Custom format with modifiers
    #[cfg_attr(feature = "schemars", schemars(title = "CustomModifiers"))]
    CustomModifiers(CustomModifiers),
}
```

**Schema Output:**
```json
{
  "description": "Date widget format",
  "oneOf": [
    { "description": "Month/Date/Year format", "type": "string", "const": "MonthDateYear" },
    { "description": "Year-Month-Date format", "type": "string", "const": "YearMonthDate" },
    {
      "title": "Custom",
      "description": "Custom format",
      "type": "object",
      "properties": {
        "Custom": { "type": "string" }
      },
      "additionalProperties": false,
      "required": ["Custom"]
    },
    {
      "title": "CustomModifiers",
      "type": "object",
      "properties": {
        "CustomModifiers": { "$ref": "#/$defs/CustomModifiers" }
      },
      "additionalProperties": false,
      "required": ["CustomModifiers"]
    }
  ]
}
```

**Title/Discriminant Location:**
| Variant Type | Field | JSON Path | Description |
|--------------|-------|-----------|-------------|
| Unit | Variant Name | `oneOf[n].const` | The variant name as a string constant |
| Unit | Description | `oneOf[n].description` | Doc comment from the variant |
| Newtype | Title | `oneOf[n].title` | Variant name (from `#[schemars(title = "...")]`) |
| Newtype | Property Key | `oneOf[n].properties.<VariantName>` | Variant name as object property key |
| Newtype | Required | `oneOf[n].required[0]` | Variant name in required array |

**Examples in schema.bar.json:**
| Definition | Lines |
|------------|-------|
| `DateFormat` | L2137-2189 |
| `TimeFormat` | L3442-3490 |
| `KomorebiLayout` | L2794-2832 |

**Examples in schema.json:**
| Definition | Lines | Notes |
|------------|-------|-------|
| `AnimationStyle` | L312-505 | 31 unit variants + `CubicBezier(f64,f64,f64,f64)` - **Note: Has custom Serialize/Deserialize that serializes CubicBezier as bare array, but schema shows wrapped object** |

---

## Pattern 4: Untagged Enum (anyOf)

**Rust Pattern:** Enum with `#[serde(untagged)]` attribute.

**JSONSchema Representation:** Uses `anyOf` (not `oneOf`) with each variant's type directly, without wrapper objects or discriminator fields.

### Subpattern 4a: Untagged with Primitives

**Source Example:** `MonitorConfigOrIndex` in `komorebi-bar/src/config.rs`
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum MonitorConfigOrIndex {
    /// The monitor index
    Index(usize),
    /// The full monitor config
    MonitorConfig(MonitorConfig),
}
```

**Schema Output:**
```json
{
  "description": "Monitor configuration or monitor index",
  "anyOf": [
    { "description": "The monitor index", "type": "integer", "format": "uint", "minimum": 0 },
    { "description": "The full monitor config", "$ref": "#/$defs/MonitorConfig" }
  ]
}
```

### Subpattern 4b: Untagged with Multiple Struct Refs

**Source Example:** `SpacingKind` in `komorebi-bar/src/config.rs`
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
// WARNING: Order matters! Grouped must come last
pub enum SpacingKind {
    /// Spacing applied to all sides
    All(f32),
    /// Individual spacing
    Individual(IndividualSpacingConfig),
    /// Grouped spacing
    Grouped(GroupedSpacingConfig),
}
```

**Schema Output:**
```json
{
  "description": "Spacing kind",
  "anyOf": [
    { "description": "Spacing applied to all sides", "type": "number", "format": "float" },
    { "description": "Individual spacing", "$ref": "#/$defs/IndividualSpacingConfig" },
    { "description": "Grouped spacing", "$ref": "#/$defs/GroupedSpacingConfig" }
  ]
}
```

### Subpattern 4c: Untagged with Two $refs

**Source Example:** `Colour` in `komorebi/src/core/colour.rs`
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum Colour {
    /// RGB representation
    Rgb(Rgb),
    /// Hex representation
    Hex(Hex),
}
```

**Schema Output:**
```json
{
  "description": "Colour representation",
  "anyOf": [
    { "description": "Colour represented as RGB", "$ref": "#/$defs/Rgb" },
    { "description": "Colour represented as Hex", "$ref": "#/$defs/Hex" }
  ]
}
```

### Subpattern 4d: Untagged with Single vs Array

**Source Example:** `MatchingRule` in `komorebi/src/core/config_generation.rs`
```rust
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum MatchingRule {
    /// Simple matching rule
    Simple(IdWithIdentifier),
    /// Composite matching rule
    Composite(Vec<IdWithIdentifier>),
}
```

**Schema Output:**
```json
{
  "description": "Rule for matching applications",
  "anyOf": [
    { "description": "Simple matching rule", "$ref": "#/$defs/IdWithIdentifier" },
    { "description": "Composite matching rule", "type": "array", "items": { "$ref": "#/$defs/IdWithIdentifier" } }
  ]
}
```

### Subpattern 4e: Untagged with Tuple

**Source Example:** `GroupedSpacingOptions` in `komorebi-bar/src/config.rs`
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum GroupedSpacingOptions {
    /// Symmetrical spacing
    Symmetrical(f32),
    /// Split spacing (top/left, bottom/right)
    Split(f32, f32),
}
```

**Schema Output:**
```json
{
  "description": "Grouped spacing options",
  "anyOf": [
    { "description": "Symmetrical spacing", "type": "number", "format": "float" },
    {
      "description": "Split spacing",
      "type": "array",
      "maxItems": 2,
      "minItems": 2,
      "prefixItems": [
        { "type": "number", "format": "float" },
        { "type": "number", "format": "float" }
      ]
    }
  ]
}
```

**Title/Discriminant Location:**
| Field | JSON Path | Description |
|-------|-----------|-------------|
| Variant Description | `anyOf[n].description` | Doc comment from the variant |
| Variant Title (optional) | `anyOf[n].title` | Title if `#[schemars(title = "...")]` is used |
| **No discriminant** | N/A | Untagged enums have no discriminant field - they rely on type matching |

**Note:** Untagged enums use `anyOf` (not `oneOf`) and do not have explicit discriminant values. The JSON parser tries each variant in order until one matches the input structure.

**All Untagged Enum Examples:**
| Schema | Definition | Lines | Subpattern |
|--------|------------|-------|------------|
| schema.json | `Colour` | L2397-2409 | 4c |
| schema.json | `MatchingRule` | L2882-2897 | 4d |
| schema.json | `AspectRatio` | L595-621 | 4a (ref + tuple) |
| schema.json | `AppSpecificConfigurationPath` | L554-569 | 4d (ref + array) |
| schema.bar.json | `MonitorConfigOrIndex` | L3053-3067 | 4a |
| schema.bar.json | `SpacingKind` | L3310-3327 | 4b |
| schema.bar.json | `GroupedSpacingOptions` | L2312-2337 | 4e |
| schema.bar.json | `RoundingConfig` | L3290-3309 | 4e |

---

## Pattern 5: Internally Tagged Enum (Struct Variants with Discriminator)

**Rust Pattern:** Enum with `#[serde(tag = "field_name")]` where variants have named fields (struct variants).

**JSONSchema Representation:** Uses `oneOf` where each variant is an object with:
- A discriminator field with `{ "type": "string", "const": "VariantName" }`
- All other fields from the struct variant
- The discriminator field in `required` array

**Source Example:** `KomorebiTheme` in `komorebi/src/static_config.rs`
```rust
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "palette")]
pub enum KomorebiTheme {
    /// A theme from catppuccin-egui
    #[cfg_attr(feature = "schemars", schemars(title = "Catppuccin"))]
    Catppuccin {
        name: komorebi_themes::Catppuccin,
        #[serde(skip_serializing_if = "Option::is_none")]
        single_border: Option<komorebi_themes::CatppuccinValue>,
        // ... more fields
    },
    /// A theme from base16-egui-themes
    #[cfg_attr(feature = "schemars", schemars(title = "Base16"))]
    Base16 {
        name: komorebi_themes::Base16,
        // ... fields
    },
    /// A custom Base16 theme
    #[cfg_attr(feature = "schemars", schemars(title = "Custom"))]
    Custom {
        colours: Base16ColourPalette,
        // ... fields
    },
}
```

**Schema Output:**
```json
{
  "description": "Komorebi theme",
  "oneOf": [
    {
      "title": "Catppuccin",
      "description": "A theme from catppuccin-egui",
      "type": "object",
      "properties": {
        "palette": { "type": "string", "const": "Catppuccin" },
        "name": { "$ref": "#/$defs/Catppuccin" },
        "single_border": { "anyOf": [{ "$ref": "#/$defs/CatppuccinValue" }, { "type": "null" }], "default": "Blue" }
      },
      "required": ["palette", "name"]
    },
    {
      "title": "Base16",
      "type": "object",
      "properties": {
        "palette": { "type": "string", "const": "Base16" },
        "name": { "$ref": "#/$defs/Base16" }
      },
      "required": ["palette", "name"]
    },
    {
      "title": "Custom",
      "type": "object",
      "properties": {
        "palette": { "type": "string", "const": "Custom" },
        "colours": { "$ref": "#/$defs/Base16ColourPalette" }
      },
      "required": ["palette", "colours"]
    }
  ]
}
```

**Title/Discriminant Location:**
| Field | JSON Path | Description |
|-------|-----------|-------------|
| Variant Title | `oneOf[n].title` | Variant name (from `#[schemars(title = "...")]`) |
| **Discriminant Field** | `oneOf[n].properties.<tag_name>` | The tag field (e.g., `palette`) as a property |
| **Discriminant Value** | `oneOf[n].properties.<tag_name>.const` | The variant name as const value |
| Required Fields | `oneOf[n].required` | Contains the tag field name |

**Note:** The discriminant field name comes from `#[serde(tag = "...")]`. In this case, `palette` is the tag field, and each variant has `properties.palette.const` set to its variant name.

**Examples:**
| Schema | Definition | Discriminator | Lines |
|--------|------------|---------------|-------|
| schema.json | `KomorebiTheme` | `palette` | L2540-2853 |
| schema.bar.json | `KomobarTheme` | `palette` | L2518-2669 |

---

## Pattern 6: Internally Tagged Enum with Flattened Newtype Variants

**Rust Pattern:** Enum with `#[serde(tag = "kind")]` where some variants wrap a struct (newtype pattern). The wrapped struct's fields get flattened.

**JSONSchema Representation:** Uses `oneOf` where:
- Each variant has a discriminator field (`kind`)
- Variants that wrap structs include `$ref` at the same level to merge properties

**Source Example:** `Grouping` in `komorebi-bar/src/render.rs`
```rust
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "kind")]
pub enum Grouping {
    /// No grouping
    #[cfg_attr(feature = "schemars", schemars(title = "None"))]
    None,
    /// Grouped as a whole
    #[cfg_attr(feature = "schemars", schemars(title = "Bar"))]
    Bar(GroupingConfig),
    /// Grouped by alignment
    #[cfg_attr(feature = "schemars", schemars(title = "Alignment"))]
    Alignment(GroupingConfig),
    /// Grouped individually
    #[cfg_attr(feature = "schemars", schemars(title = "Widget"))]
    Widget(GroupingConfig),
}
```

**Schema Output:**
```json
{
  "description": "Grouping",
  "oneOf": [
    {
      "title": "None",
      "description": "No grouping is applied",
      "type": "object",
      "properties": {
        "kind": { "type": "string", "const": "None" }
      },
      "required": ["kind"]
    },
    {
      "title": "Bar",
      "description": "Widgets are grouped as a whole",
      "type": "object",
      "properties": {
        "kind": { "type": "string", "const": "Bar" }
      },
      "$ref": "#/$defs/GroupingConfig",
      "required": ["kind"]
    }
  ]
}
```

**Title/Discriminant Location:**
| Field | JSON Path | Description |
|-------|-----------|-------------|
| Variant Title | `oneOf[n].title` | Variant name (from `#[schemars(title = "...")]`) |
| **Discriminant Field** | `oneOf[n].properties.<tag_name>` | The tag field (e.g., `kind`) as a property |
| **Discriminant Value** | `oneOf[n].properties.<tag_name>.const` | The variant name as const value |
| Flattened Properties | `oneOf[n].$ref` | Reference to merged struct (only for newtype variants) |
| Required Fields | `oneOf[n].required` | Contains the tag field name |

**Note:** Similar to Pattern 5, but newtype variants include a `$ref` at the variant level that merges in additional properties from the wrapped struct. The `$ref` appears alongside `properties` and `required`.

**Examples:**
| Schema | Definition | Discriminator | Lines |
|--------|------------|---------------|-------|
| schema.bar.json | `Grouping` | `kind` | L2338-2401 |

---

## Pattern 7: Externally Tagged Enum (Newtype Variants Only)

**Rust Pattern:** Enum with only newtype variants (no unit variants), using default external tagging.

**JSONSchema Representation:** Uses `oneOf` where each variant is an object with:
- The variant name as a property key
- The wrapped type as the property value (usually a `$ref`)
- `additionalProperties: false`
- The variant name in `required` array

**Source Example:** `WidgetConfig` in `komorebi-bar/src/widgets/widget.rs`
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
pub enum WidgetConfig {
    /// Applications widget
    #[cfg_attr(feature = "schemars", schemars(title = "Applications"))]
    Applications(ApplicationsConfig),
    /// Battery widget
    #[cfg_attr(feature = "schemars", schemars(title = "Battery"))]
    Battery(BatteryConfig),
    // ... 8 more variants
}
```

**Schema Output:**
```json
{
  "description": "Widget configuration",
  "oneOf": [
    {
      "title": "Applications",
      "description": "Applications widget configuration",
      "type": "object",
      "properties": {
        "Applications": { "$ref": "#/$defs/ApplicationsConfig" }
      },
      "additionalProperties": false,
      "required": ["Applications"]
    },
    {
      "title": "Battery",
      "type": "object",
      "properties": {
        "Battery": { "$ref": "#/$defs/BatteryConfig" }
      },
      "additionalProperties": false,
      "required": ["Battery"]
    }
  ]
}
```

**Title/Discriminant Location:**
| Field | JSON Path | Description |
|-------|-----------|-------------|
| Variant Title | `oneOf[n].title` | Variant name (from `#[schemars(title = "...")]`) |
| **Discriminant (Property Key)** | `oneOf[n].properties.<VariantName>` | Variant name is the property key itself |
| **Discriminant (Required)** | `oneOf[n].required[0]` | Variant name in required array |
| Wrapped Type | `oneOf[n].properties.<VariantName>.$ref` | Reference to the wrapped struct |

**Note:** In externally tagged enums, the variant name serves as both the property key and the discriminant. There is no separate `const` field - the presence of the property key identifies the variant.

**Examples:**
| Schema | Definition | Lines |
|--------|------------|-------|
| schema.bar.json | `WidgetConfig` | L3525-3669 |

---

## Pattern 8: Adjacently Tagged Enum

**Rust Pattern:** Enum with `#[serde(tag = "type", content = "content")]` - tag and content in separate fields.

**JSONSchema Representation:** This pattern is used for internal message types but may not appear in the public config schemas.

**Source Example:** `SocketMessage` in `komorebi/src/core/mod.rs`
```rust
#[serde_with::serde_as]
#[derive(Clone, Debug, Serialize, Deserialize, Display)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(tag = "type", content = "content")]
pub enum SocketMessage {
    FocusWindow(OperationDirection),
    MoveWindow(OperationDirection),
    CancelPreselect,
    // ... many more variants
}
```

**Title/Discriminant Location:**
| Field | JSON Path | Description |
|-------|-----------|-------------|
| **Tag Field** | `oneOf[n].properties.<tag_name>` | The tag field (e.g., `type`) |
| **Tag Value** | `oneOf[n].properties.<tag_name>.const` | The variant name as const value |
| **Content Field** | `oneOf[n].properties.<content_name>` | The content field (e.g., `content`) |
| Content Value | `oneOf[n].properties.<content_name>.*` | The variant's data |

**Note:** `SocketMessage`, `WindowManagerEvent`, and `SystemNotification` use this pattern but are primarily for IPC, not user configuration. They may not appear in the public schema files.

---

## Pattern 9: Untagged Hybrid Enum (Unit Variants + $ref Fallback)

**Rust Pattern:** Macro-generated enum using `extend_enum!` with `#[serde(untagged)]` on the fallback variant.

**Source Example:** `WorkspacesDisplayFormat` in `komorebi-bar/src/config.rs`
```rust
extend_enum!(
    DisplayFormat, WorkspacesDisplayFormat, {
    /// Show all icons only
    AllIcons,
    /// Show both all icons and text
    AllIconsAndText,
    /// Show all icons and text for the selected element
    AllIconsAndTextOnSelected,
});
```

Which expands to include an `#[serde(untagged)] Existing(DisplayFormat)` variant.

**Schema Output:**
```json
{
  "description": "Workspaces display format",
  "anyOf": [
    { "description": "Show all icons only", "type": "string", "const": "AllIcons" },
    { "description": "Show both all icons and text", "type": "string", "const": "AllIconsAndText" },
    { "description": "Show all icons and text for the selected element", "type": "string", "const": "AllIconsAndTextOnSelected" },
    { "$ref": "#/$defs/DisplayFormat" }
  ]
}
```

**Title/Discriminant Location:**
| Variant Type | Field | JSON Path | Description |
|--------------|-------|-----------|-------------|
| Unit | Variant Name | `anyOf[n].const` | The variant name as a string constant |
| Unit | Description | `anyOf[n].description` | Doc comment from the variant |
| Fallback | Reference | `anyOf[n].$ref` | Reference to the base enum for additional variants |

**Note:** This hybrid pattern combines explicit unit variants with a `$ref` fallback to another enum. The `$ref` variant has no `const` - it delegates to the referenced enum's schema.

**Examples:**
| Schema | Definition | Lines |
|--------|------------|-------|
| schema.bar.json | `WorkspacesDisplayFormat` | L3670-3692 |

---

## Pattern 10: Generic Enum Instantiations

**Rust Pattern:** Generic enum like `PerAnimationPrefixConfig<T>` with `#[serde(untagged)]` instantiated with different type parameters.

**JSONSchema Representation:** Creates separate definitions for each instantiation, with numbered suffixes.

**Source Example:** (likely in `komorebi/src/animation/mod.rs`)
```rust
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(feature = "schemars", derive(schemars::JsonSchema))]
#[serde(untagged)]
pub enum PerAnimationPrefixConfig<T> {
    /// Prefixed config
    Prefix(HashMap<String, T>),
    /// Global config
    Global(T),
}
```

**Schema Output (for bool):**
```json
{
  "description": "Animation configuration",
  "anyOf": [
    {
      "description": "Prefixed config",
      "type": "object",
      "additionalProperties": { "type": "boolean" }
    },
    {
      "description": "Global config",
      "type": "boolean"
    }
  ]
}
```

**Title/Discriminant Location:**
| Field | JSON Path | Description |
|-------|-----------|-------------|
| Variant Description | `anyOf[n].description` | Doc comment from the variant |
| **No discriminant** | N/A | Untagged - relies on type matching |
| Definition Name | `$defs/<Name>` or `$defs/<Name><N>` | Numbered suffix for each instantiation |

**Note:** Each generic instantiation creates a separate definition. The first uses the base name, subsequent ones get numeric suffixes (e.g., `PerAnimationPrefixConfig2`, `PerAnimationPrefixConfig3`).

**Examples in schema.json:**
| Definition | Type Parameter | Lines |
|------------|---------------|-------|
| `PerAnimationPrefixConfig` | `bool` | L3089-3104 |
| `PerAnimationPrefixConfig2` | `u64` | L3105-3124 |
| `PerAnimationPrefixConfig3` | `AnimationStyle` | L3125-3140 |

---

## Summary Table

| Pattern | Serde Attribute | JSONSchema Keyword | Discriminant Location |
|---------|-----------------|-------------------|----------------------|
| 1. Simple Unit | (none) | `oneOf` | `oneOf[n].const` |
| 2. Unit + Default | `#[schemars(extend)]` | `oneOf` + `default` | `oneOf[n].const` + root `default` |
| 3. Mixed (Unit + Newtype) | (none) | `oneOf` | Units: `oneOf[n].const`; Newtypes: `oneOf[n].properties.<Name>` + `oneOf[n].required[0]` |
| 4. Untagged | `#[serde(untagged)]` | `anyOf` | None (type-based matching) |
| 5. Internally Tagged | `#[serde(tag = "...")]` | `oneOf` | `oneOf[n].properties.<tag>.const` |
| 6. Internally Tagged + Newtype | `#[serde(tag = "...")]` | `oneOf` with `$ref` | `oneOf[n].properties.<tag>.const` + `oneOf[n].$ref` |
| 7. Externally Tagged (Newtype) | (none/default) | `oneOf` | `oneOf[n].properties.<VariantName>` + `oneOf[n].required[0]` |
| 8. Adjacently Tagged | `#[serde(tag, content)]` | `oneOf` | `oneOf[n].properties.<tag>.const` + `oneOf[n].properties.<content>` |
| 9. Untagged Hybrid | `#[serde(untagged)]` on fallback | `anyOf` | Units: `anyOf[n].const`; Fallback: `anyOf[n].$ref` |
| 10. Generic | `#[serde(untagged)]` | `anyOf` | None; Definition name has numeric suffix |

### Title Location by Pattern

| Pattern | Title Location | Notes |
|---------|---------------|-------|
| 1. Simple Unit | No `title` field | Variant name is in `const` |
| 2. Unit + Default | No `title` field | Variant name is in `const` |
| 3. Mixed | `oneOf[n].title` (newtypes only) | From `#[schemars(title = "...")]` |
| 4. Untagged | `anyOf[n].title` (optional) | Rarely used |
| 5. Internally Tagged | `oneOf[n].title` | From `#[schemars(title = "...")]` |
| 6. Internally Tagged + Newtype | `oneOf[n].title` | From `#[schemars(title = "...")]` |
| 7. Externally Tagged | `oneOf[n].title` | From `#[schemars(title = "...")]` |
| 8. Adjacently Tagged | `oneOf[n].title` | From `#[schemars(title = "...")]` |
| 9. Untagged Hybrid | No `title` for units | Unit names in `const`, fallback is `$ref` |
| 10. Generic | No `title` | Definition name identifies the instantiation |

---

## Key Observations

1. **`oneOf` vs `anyOf`**: Tagged enums use `oneOf` (exactly one must match); untagged enums use `anyOf` (first match wins).

2. **Unit variants**: Always represented as `{ "type": "string", "const": "VariantName" }`.

3. **Newtype variants with default tagging**: Become objects where the variant name is a property key containing the data.

4. **Struct variants with internal tagging**: The tag field appears as a regular property with a `const` value.

5. **Generic type monomorphization**: Each instantiation gets a separate schema definition with numeric suffixes.

6. **Descriptions preserved**: Both enum-level and variant-level doc comments appear in the schema.

7. **`#[schemars(title = "...")]`**: Used to control the `title` field in schema output for variants.

8. **Custom Serialize/Deserialize**: When enums have custom implementations (like `AnimationStyle`), the schema may not match the actual serialization format exactly.

9. **Variant ordering in untagged enums**: Order matters! See `SpacingKind` warning comment.

10. **Optional fields in struct variants**: Use `anyOf: [{ $ref }, { type: null }]` pattern with `default` values.

---

## Source Files Reference

| File | Key Enums |
|------|-----------|
| `komorebi/src/core/mod.rs` | `ApplicationIdentifier`, `SocketMessage`, `OperationBehaviour`, `WindowContainerBehaviour`, `FloatingLayerBehaviour`, `Placement`, `MoveBehaviour`, `CrossBoundaryBehaviour` |
| `komorebi/src/core/config_generation.rs` | `MatchingRule`, `MatchingStrategy`, `ApplicationOptions` |
| `komorebi/src/core/default_layout.rs` | `DefaultLayout` |
| `komorebi/src/core/animation.rs` | `AnimationStyle` |
| `komorebi/src/static_config.rs` | `KomorebiTheme` |
| `komorebi/src/animation/mod.rs` | `PerAnimationPrefixConfig<T>` |
| `komorebi-bar/src/config.rs` | `MonitorConfigOrIndex`, `SpacingKind`, `GroupedSpacingOptions`, `KomobarTheme`, `WorkspacesDisplayFormat` |
| `komorebi-bar/src/render.rs` | `Grouping`, `GroupingStyle`, `RoundingConfig` |
| `komorebi-bar/src/widgets/widget.rs` | `WidgetConfig` |
| `komorebi-bar/src/widgets/date.rs` | `DateFormat` |
| `komorebi-bar/src/widgets/time.rs` | `TimeFormat` |
