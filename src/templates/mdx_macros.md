{# ============================================================================ #}
{# MDX Macros for Astro Starlight documentation generation                     #}
{# ============================================================================ #}
{# 
   Enum Pattern Reference (see ENUMS.md for full details):
   - Pattern 1: Simple Unit Enums - oneOf with {type: "string", const: "Value"}
   - Pattern 2: Unit Enum with Default - Pattern 1 + "default" field
   - Pattern 3: Mixed Enum - oneOf with unit variants + tuple/object variants
   - Pattern 4a-d: Untagged Enums - anyOf with various combinations ($ref, tuple, array)
   - Pattern 5: Internally Tagged - oneOf with discriminator field (const) + properties
   - Pattern 6: Internally Tagged + Flatten - oneOf with properties + $ref at same level
   - Pattern 7: Externally Tagged - oneOf with variant name as property key
   - Pattern 8: Untagged Hybrid - anyOf mixing const strings + $ref
   - Pattern 9: Untagged with Nulls - anyOf with multiple {type: "null"} variants
   - Pattern 10: Generic Enum - anyOf with {additionalProperties: T} + T
   - Pattern 11: Nullable Optional - anyOf with $ref + {type: "null"} + default
#}

{# ============================================================================ #}
{# SECTION 1: Type Badge Rendering                                             #}
{# ============================================================================ #}
{# Render the type badge for a schema - outputs inline code or links           #}
{# When node has $ref, always prioritize it over type (Pattern 6, etc.)        #}

{% macro render_type_badge(node) %}
{%- if node["$ref"] -%}
  {# $ref always takes priority - render as link to type definition #}
  {%- set ref_name = node["$ref"] | replace(from="#/$defs/", to="") -%}
  {%- set display = display_names[ref_name] | default(value=ref_name) -%}
  {%- if schema["$defs"][ref_name] -%}
    {%- set ref_def = schema["$defs"][ref_name] -%}
[`{{ display }}`](#{{ ref_name | lower }}){% if ref_def._wrapper_inner %} {{ self::render_type_badge(node=ref_def._wrapper_inner) }}{% endif -%}
  {%- else -%}
`$ref`
  {%- endif -%}
{%- elif node.type -%}
  {%- if node.type is string -%}
    {%- if node.type == "array" and node.prefixItems -%}
      {# Check if all prefixItems have the same type (homogeneous tuple) #}
      {%- set first_type = node.prefixItems | first -%}
      {%- set_global is_homogeneous = true -%}
      {%- for item in node.prefixItems -%}
        {%- if item.type is defined and first_type.type is defined -%}
          {%- if item.type != first_type.type -%}
            {%- set_global is_homogeneous = false -%}
          {%- endif -%}
        {%- else -%}
          {%- set_global is_homogeneous = false -%}
        {%- endif -%}
      {%- endfor -%}
      {%- if is_homogeneous and first_type.type -%}
`array` `[{% if first_type.format %}{{ first_type.format }}{% else %}{{ first_type.type }}{% endif %}; {{ node.prefixItems | length }}]`
      {%- else -%}
`array` `tuple`
      {%- endif -%}
    {%- elif node.type == "array" and node.items -%}
`array` {{ self::render_type_badge(node=node.items) }}
    {%- else -%}
`{{ node.type }}`
    {%- endif -%}
  {%- else -%}
    {# Check for Option<T> pattern: type array with exactly 2 types where one is null #}
    {%- if node.type | length == 2 and "null" in node.type -%}
<Badge text="optional" variant="note" /> {% for t in node.type %}{% if t != "null" %}{% if t == "array" and node.items %}`array` {{ self::render_type_badge(node=node.items) }}{% else %}`{{ t }}`{% endif %}{% endif %}{% endfor %}
    {%- else -%}
{% for t in node.type %}`{{ t }}` {% endfor %}
    {%- endif -%}
  {%- endif -%}
{%- elif node.anyOf -%}
  {# Pattern 10: Wrapper type (detected in prepare.rs) #}
  {%- if node._wrapper_type -%}
{{ self::render_type_badge(node=node._wrapper_inner) }}
  {%- elif node.anyOf | length == 2 -%}
    {%- set first = node.anyOf | first -%}
    {%- set second = node.anyOf | last -%}
    {# Pattern 11: Nullable Optional - anyOf with T + null #}
    {%- if second.type and second.type == "null" -%}
<Badge text="optional" variant="note" /> {{ self::render_type_badge(node=first) }}
    {%- elif first.type and first.type == "null" -%}
<Badge text="optional" variant="note" /> {{ self::render_type_badge(node=second) }}
    {%- else -%}
      {# Pattern 8: Check if all variants are string const (untagged hybrid) #}
      {%- set_global is_string_enum = true -%}
      {%- for variant in node.anyOf -%}
        {%- if variant.type and variant.type == "string" and variant.const -%}
        {%- else -%}
          {%- set_global is_string_enum = false -%}
        {%- endif -%}
      {%- endfor -%}
      {%- if is_string_enum -%}
`string` `enum`
      {%- else -%}
`enum`
      {%- endif -%}
    {%- endif -%}
  {%- else -%}
    {# Pattern 4a-d, 8, 9: Untagged enums with 3+ variants #}
    {%- set_global is_string_enum = true -%}
    {%- for variant in node.anyOf -%}
      {%- if variant.type and variant.type == "string" and variant.const -%}
      {%- else -%}
        {%- set_global is_string_enum = false -%}
      {%- endif -%}
    {%- endfor -%}
    {%- if is_string_enum -%}
`string` `enum`
    {%- else -%}
`enum`
    {%- endif -%}
  {%- endif -%}
{%- elif node.oneOf -%}
  {# Pattern 1-3, 5-7: Tagged enums using oneOf #}
  {%- set_global is_string_enum = true -%}
  {%- for variant in node.oneOf -%}
    {# Pattern 1-2: Simple unit enum with string const #}
    {%- if variant.type and variant.type == "string" and variant.const -%}
    {%- else -%}
      {# Pattern 3, 5, 6, 7: Complex variants (objects, refs, etc.) #}
      {%- set_global is_string_enum = false -%}
    {%- endif -%}
  {%- endfor -%}
  {%- if is_string_enum -%}
`string` `enum`
  {%- else -%}
`enum`
  {%- endif -%}
{%- elif node.allOf -%}
`allOf`
{%- elif node.const -%}
`const`
{%- elif node.enum -%}
`enum`
{%- endif -%}
{% endmacro render_type_badge %}

{# ============================================================================ #}
{# SECTION 2: Constraint Rendering                                             #}
{# ============================================================================ #}
{# Render constraints like min, max, pattern, format                           #}

{% macro render_constraints(node) %}
{# Check if this is a homogeneous tuple - skip minItems/maxItems if so #}
{%- set_global skip_item_constraints = false -%}
{%- if node.prefixItems -%}
  {%- set first_type = node.prefixItems | first -%}
  {%- set_global is_homogeneous = true -%}
  {%- for item in node.prefixItems -%}
    {%- if item.type is defined and first_type.type is defined -%}
      {%- if item.type != first_type.type -%}
        {%- set_global is_homogeneous = false -%}
      {%- endif -%}
    {%- else -%}
      {%- set_global is_homogeneous = false -%}
    {%- endif -%}
  {%- endfor -%}
  {%- if is_homogeneous and first_type.type -%}
    {%- set_global skip_item_constraints = true -%}
  {%- endif -%}
{%- endif -%}
{%- if node.format or node.pattern or node.minimum is defined or node.maximum is defined or node.minLength is defined or node.maxLength is defined or (node.minItems is defined and not skip_item_constraints) or (node.maxItems is defined and not skip_item_constraints) %}

{% if node.format %}- **Format:** `{{ node.format }}`
{% endif %}{% if node.pattern %}- **Pattern:** `{{ node.pattern }}`
{% endif %}{% if node.minimum is defined %}- **Minimum:** {{ node.minimum }}
{% endif %}{% if node.maximum is defined %}- **Maximum:** {{ node.maximum }}
{% endif %}{% if node.minLength is defined %}- **Min length:** {{ node.minLength }}
{% endif %}{% if node.maxLength is defined %}- **Max length:** {{ node.maxLength }}
{% endif %}{% if node.minItems is defined and not skip_item_constraints %}- **Min items:** {{ node.minItems }}
{% endif %}{% if node.maxItems is defined and not skip_item_constraints %}- **Max items:** {{ node.maxItems }}
{% endif %}
{%- endif -%}
{% endmacro render_constraints %}

{# ============================================================================ #}
{# SECTION 3: Schema Body Rendering                                            #}
{# ============================================================================ #}
{# Render the body of a schema (properties, items, variants, etc.)             #}

{% macro render_schema_body(node, depth, path) %}
{%- if depth < 10 -%}
{%- if node["$ref"] -%}
  {# Reference only, already shown as type badge #}
{%- elif node.properties -%}
  {%- set required_arr = node.required | default(value=[]) -%}

<details open>
<summary>Properties ({{ node.properties | length }})</summary>

{# First render required properties #}
{% for prop_name, prop in node.properties -%}
{%- if prop_name in required_arr -%}
{%- set is_required = true -%}
{{ self::render_nested_property(name=prop_name, node=prop, required=is_required, depth=depth + 1, path=path ~ "." ~ prop_name) }}
{% endif -%}
{%- endfor -%}
{# Then render optional properties #}
{% for prop_name, prop in node.properties -%}
{%- if prop_name not in required_arr -%}
{%- set is_required = false -%}
{{ self::render_nested_property(name=prop_name, node=prop, required=is_required, depth=depth + 1, path=path ~ "." ~ prop_name) }}
{% endif -%}
{%- endfor %}
</details>
{%- elif node.patternProperties -%}

{% for pattern, prop in node.patternProperties %}
{# Convert known patterns to friendly type names #}
{# Note: patterns use single backslash in the parsed JSON value #}
{% if pattern == "^\d+$" or pattern == "^[0-9]+$" or pattern == "^-?\d+$" or pattern == "^-?[0-9]+$" -%}
**Key:** `"<integer>"` **→** {{ self::render_type_badge(node=prop) }}
{% elif pattern == ".*" or pattern == "^.*$" -%}
**Key:** `"<string>"` **→** {{ self::render_type_badge(node=prop) }}
{% else -%}
**Pattern:** `{{ pattern }}` **→** {{ self::render_type_badge(node=prop) }}
{% endif %}
{% if prop.description %}{{ prop.description | mdx_escape }}{% endif %}
{{ self::render_schema_body(node=prop, depth=depth + 1, path=path ~ ".[key]") }}
{% endfor %}
{%- elif node.items -%}
  {%- if node.items["$ref"] -%}
    {# Already shown in type badge #}
  {%- elif node.items.type and node.items.type is string -%}
    {# Simple type - already shown in type badge #}
  {%- else -%}

<details open>
<summary>Array Items</summary>

{{ self::render_schema(node=node.items, depth=depth + 1, path=path ~ "[]") }}
</details>
  {%- endif -%}
{%- elif node.prefixItems -%}
  {# Check if homogeneous tuple #}
  {%- set first_type = node.prefixItems | first -%}
  {%- set_global is_homogeneous = true -%}
  {%- for item in node.prefixItems -%}
    {%- if item.type is defined and first_type.type is defined -%}
      {%- if item.type != first_type.type -%}
        {%- set_global is_homogeneous = false -%}
      {%- endif -%}
    {%- else -%}
      {%- set_global is_homogeneous = false -%}
    {%- endif -%}
  {%- endfor -%}
  {%- if is_homogeneous and first_type.type -%}
    {# Homogeneous tuple - already shown as [T; N] in badge #}
  {%- else -%}

<details open>
<summary>Tuple Items ({{ node.prefixItems | length }})</summary>

{% for item in node.prefixItems %}
**[{{ loop.index0 }}]** {{ self::render_schema(node=item, depth=depth + 1, path=path ~ "[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
  {%- endif -%}
{%- elif node.anyOf -%}
  {# Pattern 10: Wrapper type (detected in prepare.rs) #}
  {%- if node._wrapper_type -%}
{{ self::render_schema_body(node=node._wrapper_inner, depth=depth, path=path) }}
  {%- elif node.anyOf | length == 2 -%}
    {%- set first = node.anyOf | first -%}
    {%- set second = node.anyOf | last -%}
    {# Pattern 11: Nullable Optional - unwrap the non-null variant #}
    {%- if second.type and second.type == "null" -%}
{{ self::render_schema_body(node=first, depth=depth, path=path) }}
    {%- elif first.type and first.type == "null" -%}
{{ self::render_schema_body(node=second, depth=depth, path=path) }}
    {%- else -%}
    {# Pattern 4a-d, 8: Two non-null variants #}

<details open>
<summary>Enum variants ({{ node.anyOf | length }})</summary>

{% for variant in node.anyOf %}
---
{{ self::render_enum_variant(node=variant, depth=depth + 1, path=path ~ ".anyOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
    {%- endif -%}
  {%- else -%}
  {# Pattern 4a-d, 8, 9: Untagged enums with 3+ variants #}

<details open>
<summary>Enum variants ({{ node.anyOf | length }})</summary>

{% for variant in node.anyOf %}
---
{{ self::render_enum_variant(node=variant, depth=depth + 1, path=path ~ ".anyOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
  {%- endif -%}
{%- elif node.oneOf -%}
{# Pattern 1-3, 5-7: Tagged enums using oneOf #}

<details open>
<summary>Enum variants ({{ node.oneOf | length }})</summary>

{% for variant in node.oneOf %}
---
{{ self::render_enum_variant(node=variant, depth=depth + 1, path=path ~ ".oneOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
{%- elif node.allOf -%}

<details open>
<summary>All of ({{ node.allOf | length }} schemas)</summary>

{% for variant in node.allOf %}
---
{{ self::render_schema(node=variant, depth=depth + 1, path=path ~ ".allOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
{%- elif node.enum -%}

<details open>
<summary>Enum variants ({{ node.enum | length }})</summary>

{% for value in node.enum %}- `{{ value | json_encode() }}`
{% endfor %}
</details>
{%- elif node.const -%}

**Value:** `{{ node.const | json_encode() }}`
{%- endif -%}
{%- if node.additionalProperties and node.additionalProperties is object -%}

<details open>
<summary>Additional Properties</summary>

{{ self::render_schema(node=node.additionalProperties, depth=depth + 1, path=path ~ ".[*]") }}
</details>
{%- endif -%}
{%- endif -%}
{% endmacro render_schema_body %}

{# ============================================================================ #}
{# SECTION 4: Schema Rendering                                                 #}
{# ============================================================================ #}
{# Render a schema without property wrapper                                    #}

{% macro render_schema(node, depth, path) %}
{%- if depth < 10 -%}
{# For const variants with description, show description then value on separate lines #}
{%- if node.const and node.description -%}
{{ node.description | mdx_escape }}

**Value:** `{{ node.const | json_encode() }}`
{%- elif node.const -%}
**Value:** `{{ node.const | json_encode() }}`
{%- else -%}
{{ self::render_type_badge(node=node) }}
{% if node.description %}
{{ node.description | mdx_escape }}
{% endif %}
{{ self::render_constraints(node=node) }}
{%- if node.default %}

**Default:** {% if node._default_ref and schema["$defs"][node._default_ref] %}[`{{ node.default | json_encode() }}`](#{{ node._default_ref | lower }}){% else %}`{{ node.default | json_encode() }}`{% endif %}
{% endif -%}
{{ self::render_schema_body(node=node, depth=depth, path=path) }}
{%- endif -%}
{%- endif -%}
{% endmacro render_schema %}

{# ============================================================================ #}
{# SECTION 5: Enum Variant Rendering                                           #}
{# ============================================================================ #}
{# Render an enum variant - like render_schema but uses render_enum_variant_body #}
{# for flat property display. Uses exhaustive fallback chain for variant titles. #}
{#                                                                              #}
{# Title extraction priority (see ENUMS.md for pattern details):                #}
{#   1. node.title (explicit #[schemars(title = "...")])                        #}
{#   2. node.const (Pattern 1, 2, 3 unit variants, Pattern 9 hybrid)            #}
{#   3. properties.<tag>.const (Pattern 5, 6, 8 discriminator field)            #}
{#   4. required[0] when additionalProperties=false (Pattern 7 externally tagged) #}
{#   5. $ref type name (Pattern 4, 9, 10 reference types)                       #}
{#   6. Type-based fallback (array, tuple, map, primitives)                     #}
{# ============================================================================ #}

{% macro render_enum_variant(node, depth, path) %}
{%- if depth < 10 -%}
{# Extract variant name using exhaustive fallback chain #}
{%- set_global variant_name = "" -%}

{# Priority 1: Explicit title #}
{%- if node.title -%}
  {%- set_global variant_name = node.title -%}

{# Priority 2: Const value (Pattern 1, 2, 3 unit variants, Pattern 9) #}
{%- elif node.const -%}
  {%- set_global variant_name = node.const -%}

{# Priority 3 & 4: Properties-based extraction #}
{%- elif node.properties -%}
  {# Priority 3: Discriminator const from properties (Pattern 5, 6, 8) #}
  {%- for prop_name, prop in node.properties -%}
    {%- if prop.const and variant_name == "" -%}
      {%- set_global variant_name = prop.const -%}
    {%- endif -%}
  {%- endfor -%}
  {# Priority 4: Externally tagged - property key (Pattern 7) #}
  {%- if variant_name == "" and node.additionalProperties is defined and node.additionalProperties == false -%}
    {%- if node.required and node.required | length > 0 -%}
      {%- set_global variant_name = node.required | first -%}
    {%- endif -%}
  {%- endif -%}

{# Priority 5: $ref type name (Pattern 4, 9, 10) #}
{%- elif node["$ref"] -%}
  {%- set ref_name = node["$ref"] | replace(from="#/$defs/", to="") -%}
  {%- set_global variant_name = display_names[ref_name] | default(value=ref_name) -%}

{# Priority 6: Type-based fallback #}
{%- elif node.type -%}
  {%- if node.type is string -%}
    {%- if node.type == "array" -%}
      {%- if node.items and node.items["$ref"] -%}
        {%- set item_ref = node.items["$ref"] | replace(from="#/$defs/", to="") -%}
        {%- set item_display = display_names[item_ref] | default(value=item_ref) -%}
        {%- set_global variant_name = item_display ~ " Array" -%}
      {%- elif node.prefixItems -%}
        {%- set_global variant_name = "Tuple" -%}
      {%- else -%}
        {%- set_global variant_name = "Array" -%}
      {%- endif -%}
    {%- elif node.type == "object" and node.additionalProperties -%}
      {%- set_global variant_name = "Map" -%}
    {%- else -%}
      {%- set_global variant_name = node.type | capitalize -%}
    {%- endif -%}
  {%- endif -%}
{%- endif -%}

{# Render heading if we have a variant name #}
{%- if variant_name %}
#### {{ variant_name }}
{% endif -%}

{# Render description #}
{% if node.description %}
{{ node.description | mdx_escape }}
{% endif %}

{# Render value based on variant type #}
{# First check for Pattern 7: Externally Tagged (single property with additionalProperties: false) #}
{# These are handled specially in render_enum_variant_body, so skip generic value here #}
{%- set_global is_pattern7 = false -%}
{%- if node.properties -%}
  {%- set props_count = node.properties | length -%}
  {%- if props_count == 1 and node.additionalProperties is defined and node.additionalProperties == false -%}
    {%- set_global is_pattern7 = true -%}
  {%- endif -%}
{%- endif -%}
{%- if node.const -%}
**Value:** `{{ node.const | json_encode() }}`
{%- elif not is_pattern7 and (node["$ref"] or node.type or node.anyOf or node.oneOf or node.allOf) %}
**Value:** {{ self::render_type_badge(node=node) }}
{%- endif %}

{{ self::render_constraints(node=node) }}
{%- if node.default %}

**Default:** {% if node._default_ref and schema["$defs"][node._default_ref] %}[`{{ node.default | json_encode() }}`](#{{ node._default_ref | lower }}){% else %}`{{ node.default | json_encode() }}`{% endif %}
{% endif -%}
{{ self::render_enum_variant_body(node=node, depth=depth, path=path) }}
{%- endif -%}
{% endmacro render_enum_variant %}

{# ============================================================================ #}
{# SECTION 6: Enum Variant Body Rendering                                      #}
{# ============================================================================ #}
{# Render the body of an enum variant - properties are shown directly without  #}
{# details wrapper, required first.                                            #}
{# Handles Pattern 3 (mixed), Pattern 5 (internally tagged),                   #}
{# Pattern 6 (internally tagged + flatten), Pattern 7 (externally tagged)      #}

{% macro render_enum_variant_body(node, depth, path) %}
{%- if depth < 10 -%}
{%- if node.properties -%}
  {# Check for Pattern 7: Externally Tagged Enum (single property with additionalProperties: false) #}
  {%- set props_count = node.properties | length -%}
  {%- if props_count == 1 and node.additionalProperties is defined and node.additionalProperties == false -%}
    {# Pattern 7: Externally Tagged - render variant name and value inline #}
    {%- for prop_name, prop in node.properties -%}

**Value:** `{{ prop_name }}` → {{ self::render_type_badge(node=prop) }}
    {%- endfor -%}
  {%- else -%}
  {# Pattern 5, 6: Object variants with properties (internally tagged) #}
  {# Pattern 6: Handle properties first - this covers cases where both properties and $ref exist #}
  {# Skip properties with const values (discriminator fields) - already shown as variant heading #}
  {%- set required_arr = node.required | default(value=[]) -%}

{# First render required properties (skip discriminator fields with const) #}
{% for prop_name, prop in node.properties -%}
{%- if prop_name in required_arr and not prop.const -%}
{%- set is_required = true -%}
{{ self::render_nested_property(name=prop_name, node=prop, required=is_required, depth=depth + 1, path=path ~ "." ~ prop_name) }}
{% endif -%}
{%- endfor -%}
{# Then render optional properties (skip discriminator fields with const) #}
{% for prop_name, prop in node.properties -%}
{%- if prop_name not in required_arr and not prop.const -%}
{%- set is_required = false -%}
{{ self::render_nested_property(name=prop_name, node=prop, required=is_required, depth=depth + 1, path=path ~ "." ~ prop_name) }}
{% endif -%}
{%- endfor -%}
  {%- endif -%}
{# Pattern 6: When node has both properties AND $ref, the $ref is shown as a link in Value: #}
{# so we don't need to inline the referenced type's properties here #}
{%- elif node["$ref"] -%}
  {# Reference only, already shown as type badge #}
{%- elif node.patternProperties -%}

{% for pattern, prop in node.patternProperties %}
{% if pattern == "^\d+$" or pattern == "^[0-9]+$" or pattern == "^-?\d+$" or pattern == "^-?[0-9]+$" -%}
**Key:** `"<integer>"` **→** {{ self::render_type_badge(node=prop) }}
{% elif pattern == ".*" or pattern == "^.*$" -%}
**Key:** `"<string>"` **→** {{ self::render_type_badge(node=prop) }}
{% else -%}
**Pattern:** `{{ pattern }}` **→** {{ self::render_type_badge(node=prop) }}
{% endif %}
{% if prop.description %}{{ prop.description | mdx_escape }}{% endif %}
{{ self::render_enum_variant_body(node=prop, depth=depth + 1, path=path ~ ".[key]") }}
{% endfor %}
{%- elif node.items -%}
  {%- if node.items["$ref"] -%}
    {# Already shown in type badge #}
  {%- elif node.items.type and node.items.type is string -%}
    {# Simple type - already shown in type badge #}
  {%- else -%}

<details open>
<summary>Array Items</summary>

{{ self::render_enum_variant(node=node.items, depth=depth + 1, path=path ~ "[]") }}
</details>
  {%- endif -%}
{%- elif node.prefixItems -%}
  {%- set first_type = node.prefixItems | first -%}
  {%- set_global is_homogeneous = true -%}
  {%- for item in node.prefixItems -%}
    {%- if item.type is defined and first_type.type is defined -%}
      {%- if item.type != first_type.type -%}
        {%- set_global is_homogeneous = false -%}
      {%- endif -%}
    {%- else -%}
      {%- set_global is_homogeneous = false -%}
    {%- endif -%}
  {%- endfor -%}
  {%- if is_homogeneous and first_type.type -%}
    {# Homogeneous tuple - already shown as [T; N] in badge #}
  {%- else -%}

<details open>
<summary>Tuple Items ({{ node.prefixItems | length }})</summary>

{% for item in node.prefixItems %}
**[{{ loop.index0 }}]** {{ self::render_enum_variant(node=item, depth=depth + 1, path=path ~ "[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
  {%- endif -%}
{%- elif node.anyOf -%}
  {%- if node._wrapper_type -%}
{{ self::render_enum_variant_body(node=node._wrapper_inner, depth=depth, path=path) }}
  {%- elif node.anyOf | length == 2 -%}
    {%- set first = node.anyOf | first -%}
    {%- set second = node.anyOf | last -%}
    {%- if second.type and second.type == "null" -%}
{{ self::render_enum_variant_body(node=first, depth=depth, path=path) }}
    {%- elif first.type and first.type == "null" -%}
{{ self::render_enum_variant_body(node=second, depth=depth, path=path) }}
    {%- else -%}

<details open>
<summary>Enum variants ({{ node.anyOf | length }})</summary>

{% for variant in node.anyOf %}
---
{{ self::render_enum_variant(node=variant, depth=depth + 1, path=path ~ ".anyOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
    {%- endif -%}
  {%- else -%}

<details open>
<summary>Enum variants ({{ node.anyOf | length }})</summary>

{% for variant in node.anyOf %}
---
{{ self::render_enum_variant(node=variant, depth=depth + 1, path=path ~ ".anyOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
  {%- endif -%}
{%- elif node.oneOf -%}

<details open>
<summary>Enum variants ({{ node.oneOf | length }})</summary>

{% for variant in node.oneOf %}
---
{{ self::render_enum_variant(node=variant, depth=depth + 1, path=path ~ ".oneOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
{%- elif node.allOf -%}

<details open>
<summary>All of ({{ node.allOf | length }} schemas)</summary>

{% for variant in node.allOf %}
---
{{ self::render_enum_variant(node=variant, depth=depth + 1, path=path ~ ".allOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
{%- elif node.enum -%}

<details open>
<summary>Enum variants ({{ node.enum | length }})</summary>

{% for value in node.enum %}- `{{ value | json_encode() }}`
{% endfor %}
</details>
{# Note: node.const is NOT handled here - it's already rendered in render_enum_variant #}
{%- endif -%}
{%- if node.additionalProperties and node.additionalProperties is object -%}

<details open>
<summary>Additional Properties</summary>

{{ self::render_enum_variant(node=node.additionalProperties, depth=depth + 1, path=path ~ ".[*]") }}
</details>
{%- endif -%}
{%- endif -%}
{% endmacro render_enum_variant_body %}

{# ============================================================================ #}
{# SECTION 7: Property Rendering                                               #}
{# ============================================================================ #}
{# Render a nested property (used within details blocks)                       #}

{% macro render_nested_property(name, node, required, depth, path) %}
{%- if depth < 10 -%}
{%- set anchor = path | lower | replace(from=".", to="-") -%}
#### {{ name }} {{"{"}}#{{ anchor }}{{"}"}}

{% if required %}<Badge text="required" variant="success" /> {% endif %}{% if node.deprecated %}<Badge text="deprecated" variant="danger" /> {% endif %}{{ self::render_type_badge(node=node) }}

{% if node.description %}{{ node.description | mdx_escape }}{% endif %}
{{ self::render_constraints(node=node) }}
{%- if node.default %}

**Default:** {% if node._default_ref and schema["$defs"][node._default_ref] %}[`{{ node.default | json_encode() }}`](#{{ node._default_ref | lower }}){% else %}`{{ node.default | json_encode() }}`{% endif %}
{% endif -%}
{{ self::render_schema_body(node=node, depth=depth, path=path) }}
{%- endif -%}
{% endmacro render_nested_property %}

{# Render a top-level property as H3                                           #}

{% macro render_property(name, node, required, depth, path) %}
{%- if depth < 10 -%}
{%- set anchor = path | lower | replace(from=".", to="-") -%}
### {{ name }} {{"{"}}#{{ anchor }}{{"}"}}

{% if required %}<Badge text="required" variant="success" /> {% endif %}{% if node.deprecated %}<Badge text="deprecated" variant="danger" /> {% endif %}{{ self::render_type_badge(node=node) }}

{% if node.description %}{{ node.description | mdx_escape }}{% endif %}
{{ self::render_constraints(node=node) }}
{%- if node.default %}

**Default:** {% if node._default_ref and schema["$defs"][node._default_ref] %}[`{{ node.default | json_encode() }}`](#{{ node._default_ref | lower }}){% else %}`{{ node.default | json_encode() }}`{% endif %}
{% endif -%}
{{ self::render_schema_body(node=node, depth=depth, path=path) }}

---

{%- endif -%}
{% endmacro render_property %}

{# ============================================================================ #}
{# SECTION 8: Definition & Usage Rendering                                     #}
{# ============================================================================ #}
{# Render usages section for a type definition                                 #}

{% macro render_usages(def) %}
{%- if def._usages and (def._usages.config_usages | length > 0 or def._usages.type_references | length > 0) -%}

#### Usages

{% if def._usages.config_usages | length > 0 -%}
**Configuration:**
{% for usage in def._usages.config_usages %}- [`{{ usage.path }}`](#{{ usage.anchor }})
{% endfor %}
{% endif %}
{% if def._usages.type_references | length > 0 -%}
**Referenced by:**
{% for ref in def._usages.type_references %}{%- set ref_display = display_names[ref.type_name] | default(value=ref.type_name) -%}
- [`{{ ref_display }}`](#{{ ref.anchor }})
{% endfor %}
{% endif %}
{%- endif -%}
{% endmacro render_usages %}

{# Render a type definition as H3                                              #}

{% macro render_definition(def_name, def) %}
{%- set anchor = def_name | lower | replace(from=".", to="-") -%}
### {{ def._display_name | default(value=def_name) }} {{"{"}}#{{ anchor }}{{"}"}}

{% if def.description %}{{ def.description | mdx_escape }}{% endif %}
{{ self::render_constraints(node=def) }}
{{ self::render_schema_body(node=def, depth=0, path=def_name) }}
{{ self::render_usages(def=def) }}

---

{% endmacro render_definition %}
