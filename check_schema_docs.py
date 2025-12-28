#!/usr/bin/env python3
"""
Check schema.json and schema.bar.json for missing docstrings and map them to Rust source files.

This script analyzes the generated JSON schemas and identifies:
1. Type definitions ($defs) missing top-level descriptions
2. Enum variants missing descriptions (in oneOf/anyOf)
3. Enum variants missing titles (object variants in oneOf/anyOf)
4. Struct properties missing descriptions
5. Top-level schema properties missing descriptions

For each missing docstring, it attempts to find the corresponding Rust source
file and line number where the docstring should be added.
"""

import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Optional


@dataclass
class MissingDoc:
    type_name: str
    kind: str  # "type", "variant", "property", "variant_title"
    item_name: Optional[str]  # variant or property name
    rust_file: Optional[str] = None
    rust_line: Optional[int] = None

    def __str__(self):
        location = ""
        if self.rust_file and self.rust_line:
            location = f" -> {self.rust_file}:{self.rust_line}"
        elif self.rust_file:
            location = f" -> {self.rust_file}"

        if self.kind == "type":
            return f"[TYPE] {self.type_name}{location}"
        elif self.kind == "variant":
            return f"[VARIANT] {self.type_name}::{self.item_name}{location}"
        elif self.kind == "variant_title":
            return f"[VARIANT_TITLE] {self.type_name}::{self.item_name}{location}"
        else:
            return f"[PROPERTY] {self.type_name}.{self.item_name}{location}"


@dataclass
class SchemaConfig:
    """Configuration for a schema to check."""

    schema_file: str
    search_paths: list[str]
    display_name: str


def find_rust_definition(
    type_name: str, item_name: Optional[str], kind: str, search_paths: list[Path]
) -> tuple[Optional[str], Optional[int]]:
    """Find the Rust file and line number for a type/variant/property definition."""

    if kind == "type":
        patterns = [
            rf"pub\s+enum\s+{type_name}\b",
            rf"pub\s+struct\s+{type_name}\b",
        ]
    elif kind in ("variant", "variant_title"):
        patterns = [
            rf"^\s*{re.escape(item_name)}\s*[,\(\{{]",
            rf"^\s*{re.escape(item_name)}\s*$",
            rf"^\s*#\[.*\]\s*\n\s*{re.escape(item_name)}\b",
        ]
    else:  # property
        patterns = [rf"pub\s+{re.escape(item_name)}\s*:"]

    for search_path in search_paths:
        for rust_file in search_path.rglob("*.rs"):
            try:
                content = rust_file.read_text()
                lines = content.split("\n")

                if kind == "type":
                    for pattern in patterns:
                        for i, line in enumerate(lines):
                            if re.search(pattern, line):
                                return str(rust_file), i + 1

                elif kind in ("variant", "variant_title", "property"):
                    parent_pattern = rf"pub\s+(?:enum|struct)\s+{type_name}\b"
                    in_type = False
                    brace_count = 0
                    found_open_brace = False

                    for i, line in enumerate(lines):
                        if re.search(parent_pattern, line):
                            in_type = True
                            brace_count = 0
                            found_open_brace = False

                        if in_type:
                            if "{" in line:
                                found_open_brace = True
                            brace_count += line.count("{") - line.count("}")

                            for pattern in patterns:
                                if re.search(pattern, line):
                                    return str(rust_file), i + 1

                            if found_open_brace and brace_count <= 0:
                                in_type = False
            except Exception:
                continue

    return None, None


def _get_variant_identifier(variant: dict) -> str:
    """Extract a meaningful identifier for a variant.

    Tries to find the best identifier by checking:
    1. A top-level const value (e.g., {"const": "Linear"})
    2. A property with a const value (e.g., {"kind": {"const": "Bar"}})
    3. The first required property name
    4. The type field
    5. Falls back to "unknown"
    """
    # Check for top-level const value (simple enum variant)
    if "const" in variant:
        return str(variant["const"])

    properties = variant.get("properties", {})

    # Check for a property with a const value (common pattern for tagged enums)
    for prop_name, prop_def in properties.items():
        if isinstance(prop_def, dict) and "const" in prop_def:
            return str(prop_def["const"])

    # Fall back to first required property name
    required = variant.get("required", [])
    if required:
        return str(required[0])

    # Fall back to type
    if "type" in variant:
        return str(variant["type"])

    return "unknown"


def check_type_description(type_name: str, type_def: dict) -> list[MissingDoc]:
    """Check if a type definition has proper documentation."""
    missing = []
    has_top_description = "description" in type_def

    # Always check for top-level type description first
    # (except for types that are purely references or have special handling)
    needs_type_description = True

    # Check oneOf variants (tagged enums with variant descriptions)
    if "oneOf" in type_def:
        # oneOf types should have a top-level description
        if not has_top_description:
            missing.append(MissingDoc(type_name, "type", None, None, None))

        for variant in type_def["oneOf"]:
            # Case 1: Simple const variant (e.g., {"const": "Swap", "description": "..."})
            variant_name = variant.get("const") or variant.get("title")
            if variant_name and "description" not in variant:
                missing.append(
                    MissingDoc(type_name, "variant", str(variant_name), None, None)
                )

            # Case 2: String enum inside oneOf (e.g., {"type": "string", "enum": [...]})
            # These variants don't have individual descriptions in the schema
            if "enum" in variant and variant.get("type") == "string":
                for enum_variant in variant["enum"]:
                    missing.append(
                        MissingDoc(type_name, "variant", str(enum_variant), None, None)
                    )

            # Case 3: Object variant with properties (e.g., CubicBezier)
            if "properties" in variant and "description" not in variant:
                for prop_name in variant.get("required", []):
                    missing.append(
                        MissingDoc(type_name, "variant", str(prop_name), None, None)
                    )

            # Case 4: Object variant missing title (needed for schema UI display)
            # Object variants should have a title or const for proper display in editors
            if (
                "properties" in variant
                and "title" not in variant
                and "const" not in variant
            ):
                # Try to find a good identifier for the variant (for display only)
                variant_id = _get_variant_identifier(variant)
                missing.append(
                    MissingDoc(type_name, "variant_title", str(variant_id), None, None)
                )

    # Check anyOf variants - check each variant individually
    elif "anyOf" in type_def:
        # anyOf types should have a top-level description
        if not has_top_description:
            missing.append(MissingDoc(type_name, "type", None, None, None))

        # Check each variant for description (skip pure $ref and null types)
        for variant in type_def["anyOf"]:
            # Skip null type variants (used for Option<T>)
            if variant.get("type") == "null":
                continue
            # Skip pure $ref variants (the referenced type is checked separately)
            if "$ref" in variant and len(variant) == 1:
                continue
            # Skip $ref variants that have a description
            if "$ref" in variant and "description" in variant:
                continue
            # Variant with $ref but no description
            if "$ref" in variant and "description" not in variant:
                # Extract the type name from the $ref
                ref_name = variant["$ref"].split("/")[-1]
                missing.append(MissingDoc(type_name, "variant", ref_name, None, None))
            # Non-ref variant without description
            elif "description" not in variant and "$ref" not in variant:
                # Try to identify the variant by its type or const
                variant_id = variant.get("const") or variant.get("type") or "unknown"
                missing.append(
                    MissingDoc(type_name, "variant", str(variant_id), None, None)
                )

            # Check for missing title on object variants in anyOf
            if (
                "properties" in variant
                and "title" not in variant
                and "const" not in variant
            ):
                variant_id = _get_variant_identifier(variant)
                missing.append(
                    MissingDoc(type_name, "variant_title", str(variant_id), None, None)
                )

    # Check simple string enums (no oneOf means no variant descriptions possible in schema)
    elif "enum" in type_def:
        if not has_top_description:
            missing.append(MissingDoc(type_name, "type", None, None, None))
        # Each enum variant needs a docstring - these can't have descriptions in simple enum format
        for variant in type_def["enum"]:
            missing.append(MissingDoc(type_name, "variant", str(variant), None, None))

    # Check struct properties
    elif "properties" in type_def:
        # Structs should always have a top-level description
        if not has_top_description:
            missing.append(MissingDoc(type_name, "type", None, None, None))

        for prop_name, prop_def in type_def["properties"].items():
            if "description" not in prop_def:
                missing.append(MissingDoc(type_name, "property", prop_name, None, None))

    # Simple type without description (like PathBuf, Hex)
    elif not has_top_description:
        # Only flag if it has a concrete type (not just a $ref)
        if type_def.get("type") is not None:
            missing.append(MissingDoc(type_name, "type", None, None, None))

    return missing


def check_top_level_properties(schema: dict, root_type_name: str) -> list[MissingDoc]:
    """Check top-level schema properties for missing descriptions."""
    missing = []
    properties = schema.get("properties", {})

    for prop_name, prop_def in properties.items():
        if "description" not in prop_def:
            missing.append(
                MissingDoc(root_type_name, "property", prop_name, None, None)
            )

    return missing


def check_schema(
    schema_path: Path,
    search_paths: list[Path],
    project_root: Path,
    display_name: str,
) -> tuple[list[MissingDoc], int]:
    """Check a single schema file and return missing docs and exit code."""
    if not schema_path.exists():
        print(f"Error: {schema_path.name} not found at {schema_path}")
        return [], 1

    with open(schema_path) as f:
        schema = json.load(f)

    all_missing: list[MissingDoc] = []

    # Check top-level schema properties
    root_type_name = schema.get("title", "Root")
    all_missing.extend(check_top_level_properties(schema, root_type_name))

    # Check all type definitions
    for type_name, type_def in sorted(schema.get("$defs", {}).items()):
        # Skip PerAnimationPrefixConfig2/3 as they're generated variants
        if (
            type_name.startswith("PerAnimationPrefixConfig")
            and type_name != "PerAnimationPrefixConfig"
        ):
            continue
        all_missing.extend(check_type_description(type_name, type_def))

    # Find Rust source locations
    print(f"Scanning Rust source files for {display_name}...", file=sys.stderr)
    for doc in all_missing:
        doc.rust_file, doc.rust_line = find_rust_definition(
            doc.type_name, doc.item_name, doc.kind, search_paths
        )
        if doc.rust_file:
            try:
                doc.rust_file = str(Path(doc.rust_file).relative_to(project_root))
            except ValueError:
                pass

    return all_missing, 0


def print_results(all_missing: list[MissingDoc], display_name: str) -> None:
    """Print the results for a schema check."""
    # Group by file
    by_file: dict[str, list[MissingDoc]] = {}
    external: list[MissingDoc] = []

    for doc in all_missing:
        if doc.rust_file:
            by_file.setdefault(doc.rust_file, []).append(doc)
        else:
            external.append(doc)

    # Print summary
    print("\n" + "=" * 70)
    print(f"MISSING DOCSTRINGS IN SCHEMA ({display_name})")
    print("=" * 70)

    type_count = sum(1 for d in all_missing if d.kind == "type")
    variant_count = sum(1 for d in all_missing if d.kind == "variant")
    variant_title_count = sum(1 for d in all_missing if d.kind == "variant_title")
    prop_count = sum(1 for d in all_missing if d.kind == "property")

    print(f"\nTotal: {len(all_missing)} missing docstrings/titles")
    print(f"  - {type_count} types")
    print(f"  - {variant_count} variants")
    print(f"  - {variant_title_count} variant titles")
    print(f"  - {prop_count} properties")

    # Print by file
    for rust_file in sorted(by_file.keys()):
        docs = sorted(by_file[rust_file], key=lambda d: d.rust_line or 0)
        print(f"\n{rust_file}:")
        print("-" * len(rust_file))
        for doc in docs:
            print(f"  {doc}")

    # Print external items (types not found in source)
    if external:
        print(f"\nExternal/Unknown location:")
        print("-" * 25)
        for doc in external:
            print(f"  {doc}")

    print("\n" + "=" * 70)


def main():
    project_root = Path.cwd()

    # Define schemas to check with their respective search paths
    schemas = [
        SchemaConfig(
            schema_file="schema.json",
            search_paths=["komorebi/src", "komorebi-themes/src"],
            display_name="komorebi",
        ),
        SchemaConfig(
            schema_file="schema.bar.json",
            search_paths=["komorebi-bar/src", "komorebi-themes/src"],
            display_name="komorebi-bar",
        ),
    ]

    total_missing = 0
    has_errors = False

    for schema_config in schemas:
        schema_path = project_root / schema_config.schema_file
        search_paths = [
            project_root / p
            for p in schema_config.search_paths
            if (project_root / p).exists()
        ]

        missing, error_code = check_schema(
            schema_path,
            search_paths,
            project_root,
            schema_config.display_name,
        )

        if error_code != 0:
            has_errors = True
            continue

        print_results(missing, schema_config.display_name)
        total_missing += len(missing)

    # Print combined summary
    if len(schemas) > 1:
        print("\n" + "=" * 70)
        print("COMBINED SUMMARY")
        print("=" * 70)
        print(f"Total missing docstrings across all schemas: {total_missing}")
        print("=" * 70)

    if has_errors:
        return 1

    return 1 if total_missing > 0 else 0


if __name__ == "__main__":
    sys.exit(main())
