import cadkit

MIB = 1024 * 1024

try:
    doc = cadkit.read(
        "upload.dxf",
        max_input_bytes=64 * MIB,
        max_decompressed_bytes=256 * MIB,
        max_objects=500_000,
        max_depth=32,
    )
except cadkit.LimitExceededError as e:
    print("rejected, over a limit:", e)
except cadkit.UnknownFormatError:
    print("rejected, not a drawing")
except cadkit.CadkitError as e:  # UnsupportedError, InvalidDataError
    print("rejected:", type(e).__name__, e)
else:
    print(doc, len(doc.warnings), "warnings")
