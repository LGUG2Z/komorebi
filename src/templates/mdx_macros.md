{# MDX Macros for Astro Starlight documentation generation #}
{# These macros mirror the HTML macro logic for consistent output #}

{# Render the type badge for a schema - outputs inline code or links #}
{% macro render_type_badge(node) %}
{%- if node["$ref"] -%}
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
`array` {% if first_type.type == "number" %}`[float; {{ node.prefixItems | length }}]`{% else %}`[{{ first_type.type }}; {{ node.prefixItems | length }}]`{% endif %}
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
  {# Check for wrapper type pattern first #}
  {%- if node._wrapper_type -%}
{{ self::render_type_badge(node=node._wrapper_inner) }}
  {%- elif node.anyOf | length == 2 -%}
    {%- set first = node.anyOf | first -%}
    {%- set second = node.anyOf | last -%}
    {%- if second.type and second.type == "null" -%}
<Badge text="optional" variant="note" /> {{ self::render_type_badge(node=first) }}
    {%- elif first.type and first.type == "null" -%}
<Badge text="optional" variant="note" /> {{ self::render_type_badge(node=second) }}
    {%- else -%}
      {# Check if all variants are string const (enum pattern) #}
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
    {# Check if all variants are string const (enum pattern) #}
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
  {# Check if all variants are string const (enum pattern) #}
  {%- set_global is_string_enum = true -%}
  {%- for variant in node.oneOf -%}
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
{%- elif node.allOf -%}
`allOf`
{%- elif node.const -%}
`const`
{%- elif node.enum -%}
`enum`
{%- endif -%}
{% endmacro render_type_badge %}

{# Render constraints like min, max, pattern, format #}
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

{# Render the body of a schema (properties, items, variants, etc.) #}
{% macro render_schema_body(node, depth, path) %}
{%- if depth < 10 -%}
{%- if node["$ref"] -%}
  {# Reference only, already shown as type badge #}
{%- elif node.properties -%}
  {%- set required_arr = node.required | default(value=[]) -%}

<details>
<summary>Properties ({{ node.properties | length }})</summary>

{% for prop_name, prop in node.properties -%}
{%- set is_required = prop_name in required_arr -%}
{{ self::render_nested_property(name=prop_name, node=prop, required=is_required, depth=depth + 1, path=path ~ "." ~ prop_name) }}
{% endfor %}
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

<details>
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

<details>
<summary>Tuple Items ({{ node.prefixItems | length }})</summary>

{% for item in node.prefixItems %}
**[{{ loop.index0 }}]** {{ self::render_schema(node=item, depth=depth + 1, path=path ~ "[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
  {%- endif -%}
{%- elif node.anyOf -%}
  {# Check for wrapper type pattern first #}
  {%- if node._wrapper_type -%}
{{ self::render_schema_body(node=node._wrapper_inner, depth=depth, path=path) }}
  {%- elif node.anyOf | length == 2 -%}
    {%- set first = node.anyOf | first -%}
    {%- set second = node.anyOf | last -%}
    {%- if second.type and second.type == "null" -%}
{{ self::render_schema_body(node=first, depth=depth, path=path) }}
    {%- elif first.type and first.type == "null" -%}
{{ self::render_schema_body(node=second, depth=depth, path=path) }}
    {%- else -%}

<details>
<summary>Enum variants ({{ node.anyOf | length }})</summary>

{% for variant in node.anyOf %}
---
{{ self::render_schema(node=variant, depth=depth + 1, path=path ~ ".anyOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
    {%- endif -%}
  {%- else -%}

<details>
<summary>Enum variants ({{ node.anyOf | length }})</summary>

{% for variant in node.anyOf %}
---
{{ self::render_schema(node=variant, depth=depth + 1, path=path ~ ".anyOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
  {%- endif -%}
{%- elif node.oneOf -%}

<details>
<summary>Enum variants ({{ node.oneOf | length }})</summary>

{% for variant in node.oneOf %}
---
{{ self::render_schema(node=variant, depth=depth + 1, path=path ~ ".oneOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
{%- elif node.allOf -%}

<details>
<summary>All of ({{ node.allOf | length }} schemas)</summary>

{% for variant in node.allOf %}
---
{{ self::render_schema(node=variant, depth=depth + 1, path=path ~ ".allOf[" ~ loop.index0 ~ "]") }}
{% endfor %}
</details>
{%- elif node.enum -%}

<details>
<summary>Enum variants ({{ node.enum | length }})</summary>

{% for value in node.enum %}- `{{ value | json_encode() }}`
{% endfor %}
</details>
{%- elif node.const -%}

**Value:** `{{ node.const | json_encode() }}`
{%- endif -%}
{%- if node.additionalProperties and node.additionalProperties is object -%}

<details>
<summary>Additional Properties</summary>

{{ self::render_schema(node=node.additionalProperties, depth=depth + 1, path=path ~ ".[*]") }}
</details>
{%- endif -%}
{%- endif -%}
{% endmacro render_schema_body %}

{# Render a schema without property wrapper #}
{% macro render_schema(node, depth, path) %}
{%- if depth < 10 -%}
{# Skip type badge for documented const variants - the value is shown below #}
{%- if node.const and node.description -%}
{{ node.description | mdx_escape }}
{%- else -%}
{{ self::render_type_badge(node=node) }}{% if node.description %} - {{ node.description | mdx_escape }}{% endif %}
{%- endif -%}
{{ self::render_constraints(node=node) }}
{%- if node.default %}

**Default:** {% if node._default_ref and schema["$defs"][node._default_ref] %}[`{{ node.default | json_encode() }}`](#{{ node._default_ref | lower }}){% else %}`{{ node.default | json_encode() }}`{% endif %}
{% endif -%}
{{ self::render_schema_body(node=node, depth=depth, path=path) }}
{%- endif -%}
{% endmacro render_schema %}

{# Render a nested property (used within details blocks) #}
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

{# Render a top-level property as H3 #}
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

{# Render usages section for a type definition #}
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

{# Render a type definition as H3 #}
{% macro render_definition(def_name, def) %}
{%- set anchor = def_name | lower | replace(from=".", to="-") -%}
### {{ def._display_name | default(value=def_name) }} {{"{"}}#{{ anchor }}{{"}"}}

{{ self::render_type_badge(node=def) }}

{% if def.description %}{{ def.description | mdx_escape }}{% endif %}
{{ self::render_constraints(node=def) }}
{{ self::render_schema_body(node=def, depth=0, path=def_name) }}
{{ self::render_usages(def=def) }}

---

{% endmacro render_definition %}
