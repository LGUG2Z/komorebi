{%- import "mdx_macros.md" as macros -%}
---
title: "{{ title }}"
description: "{{ description }}"
---

import { Badge } from '@astrojs/starlight/components';

## Configuration
{% if schema.properties %}
{% for prop_name, prop in schema.properties -%}
{%- set is_required = prop_name in required_set -%}
{{ macros::render_property(name=prop_name, node=prop, required=is_required, depth=0, path=prop_name) }}
{% endfor %}
{% else %}
*No configuration properties defined.*
{% endif %}
{% if schema["$defs"] %}
## Type Definitions

{% for def_name, def in schema["$defs"] -%}
{{ macros::render_definition(def_name=def_name, def=def) }}
{% endfor %}
{% endif %}
