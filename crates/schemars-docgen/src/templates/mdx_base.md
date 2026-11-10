{%- import "mdx_macros.md" as macros -%}
---
title: "{{ title }}"
description: "{{ description }}"
---

import { Badge } from '@astrojs/starlight/components';

## Configuration
{% if schema.properties %}
{# First render required properties #}
{% for prop_name, prop in schema.properties -%}
{%- if prop_name in required_set -%}
{{ macros::render_property(name=prop_name, node=prop, required=true, depth=0, path=prop_name) }}
{% endif -%}
{%- endfor -%}
{# Then render optional properties #}
{% for prop_name, prop in schema.properties -%}
{%- if prop_name not in required_set -%}
{{ macros::render_property(name=prop_name, node=prop, required=false, depth=0, path=prop_name) }}
{% endif -%}
{%- endfor %}
{% else %}
*No configuration properties defined.*
{% endif %}
{% if schema["$defs"] %}
## Type Definitions

{% for def_name, def in schema["$defs"] -%}
{{ macros::render_definition(def_name=def_name, def=def) }}
{% endfor %}
{% endif %}
