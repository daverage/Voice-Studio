#!/usr/bin/env python3
"""
Inspect VST3 parameters
"""
from pedalboard import load_plugin
import argparse

def inspect_vst(vst_path):
    """List all VST parameters with their current values"""
    print("=" * 60)
    print("VST3 Parameter Inspector")
    print("=" * 60)

    vst = load_plugin(str(vst_path))

    print(f"\nPlugin: {vst.name if hasattr(vst, 'name') else 'Unknown'}")
    print(f"Type: {type(vst)}")

    print(f"\nAll attributes and parameters:")
    print("-" * 60)

    # Get all non-private attributes
    attrs = [attr for attr in dir(vst) if not attr.startswith('_')]

    # Separate into categories
    parameters = []
    methods = []
    properties = []

    for attr in attrs:
        try:
            val = getattr(vst, attr)
            if callable(val):
                methods.append(attr)
            elif isinstance(val, (int, float)):
                parameters.append((attr, val))
            else:
                properties.append((attr, val))
        except:
            pass

    # Print parameters (these are what we can set)
    print(f"\n📊 Parameters ({len(parameters)}):")
    print("   (These are what you can modify)")
    for name, value in sorted(parameters):
        print(f"   {name:30s} = {value}")

    # Print properties
    print(f"\n📋 Properties ({len(properties)}):")
    for name, value in sorted(properties):
        val_str = str(value)[:50] if len(str(value)) <= 50 else str(value)[:47] + "..."
        print(f"   {name:30s} = {val_str}")

    # Print methods
    print(f"\n🔧 Methods ({len(methods)}):")
    print(f"   {', '.join(sorted(methods)[:10])}")
    if len(methods) > 10:
        print(f"   ... and {len(methods) - 10} more")

    print("\n" + "=" * 60)
    print("For auto_tune.py, use the parameter names exactly as shown above")
    print("=" * 60)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description='Inspect VST3 parameters')
    parser.add_argument('--vst', required=True, help='Path to VST3 plugin')
    args = parser.parse_args()

    inspect_vst(args.vst)
