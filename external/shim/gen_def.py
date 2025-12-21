import sys
import re

def generate_def(dumpbin_output_path, dll_name, version_suffix=""):
    with open(dumpbin_output_path, 'r') as f:
        lines = f.readlines()

    exports = []
    # Regex to capture ordinal, hint (optional), RVA, and symbol name
    # Example:       1    0 00001000 ??0Config@OpenColorIO_v2_5@@QEAA@XZ
    pattern = re.compile(r'^\s*(\d+)\s+[0-9A-F]+\s+[0-9A-F]+\s+(.+)$')

    for line in lines:
        match = pattern.match(line)
        if match:
            symbol = match.group(2).strip()
            # If symbol contains space (e.g. [NONAME]), skip or handle or cleaned
            if " " in symbol:
                continue
            exports.append(symbol)

    if not exports:
        print("No exports found!")
        return

    with open(f"{dll_name}.def", 'w') as f:
        f.write(f"LIBRARY {dll_name}\n")
        f.write("EXPORTS\n")
        for symbol in exports:
            f.write(f"    {symbol}\n")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: python gen_def.py <dumpbin_output_path> <dll_name>")
    else:
        generate_def(sys.argv[1], sys.argv[2])
