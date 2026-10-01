#!/usr/bin/env python3
"""Downloads the public test corpus into corpus/public/ (git-ignored).

Sources are open-source projects' own sample files; their licenses are listed
in docs/PROVENANCE.md. Files are fetched, never committed. A manifest with
SHA-256 hashes is written to corpus/public/MANIFEST.json.

    python scripts/fetch-corpus.py
"""

from __future__ import annotations

import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.request

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "corpus", "public")

ACADSHARP = "https://raw.githubusercontent.com/DomCR/ACadSharp/master/samples/"
GDAL = "https://raw.githubusercontent.com/OSGeo/gdal/master/autotest/ogr/data/"
GDAL_SEEDS = "https://raw.githubusercontent.com/OSGeo/gdal/master/ogr/ogrsf_frmts/dgn/data/"
SAFE = "https://support.safe.com/hc/article_attachments/"
TIKA = ("https://raw.githubusercontent.com/apache/tika/main/tika-parsers/tika-parsers-standard/"
        "tika-parsers-standard-modules/tika-parser-cad-module/src/test/resources/test-documents/")

FILES = {
    # ACadSharp (MIT): the same drawing saved in every DWG version plus DXF exports.
    "acadsharp": [ACADSHARP + n for n in [
        "sample_AC1014.dwg",
        "sample_AC1015.dwg", "sample_AC1015_ascii.dxf", "sample_AC1015_binary.dxf",
        "sample_AC1018.dwg", "sample_AC1018_ascii.dxf", "sample_AC1018_binary.dxf",
        "sample_AC1021.dwg", "sample_AC1021_ascii.dxf", "sample_AC1021_binary.dxf",
        "sample_AC1024.dwg", "sample_AC1024_ascii.dxf", "sample_AC1024_binary.dxf",
        "sample_AC1027.dwg", "sample_AC1027_ascii.dxf", "sample_AC1027_binary.dxf",
        "sample_AC1032.dwg", "sample_AC1032_ascii.dxf", "sample_AC1032_binary.dxf",
        "sample_AC1009_ascii.dxf", "sample_AC1009_binary.dxf",
    ]],
    # GDAL autotest data (MIT/X): DGN V7 samples and a DGN V8 sample with a reference CSV
    # produced by GDAL's ODA-based driver (usable as a black-box oracle).
    "gdal": [GDAL + n for n in [
        "dgn/smalltest.dgn",
        "dgn/knot_oob.dgn",
        "dgnv8/test_dgnv8.dgn",
        "dgnv8/test_dgnv8_ref.csv",
        "dgnv8/test_dgnv8_write_ref.csv",
    ]] + [GDAL_SEEDS + "seed_2d.dgn", GDAL_SEEDS + "seed_3d.dgn"],
    # Safe Software public support-article attachments (DGN V8 with tags / item types),
    # also used by ezdgn. Saved under fixed names; see docs/dgn/FORMAT_NOTES.md for hashes.
    "safe": [
        (SAFE + "36260727163917", "TreeTextNodeLabelsWithTags.dgn"),
        (SAFE + "36769975369869", "Water_distribution_mains.dgn"),
    ],
    # Apache Tika CAD test documents (Apache-2.0): unseen DWG files AC1015..AC1032,
    # structure-only validation (no DXF exports).
    "tika": [TIKA + n for n in [
        "architectural_-_annotation_scaling_and_multileaders.dwg",
        "testDWG-AC1027.dwg", "testDWG-AC1032.dwg", "testDWG2000.dwg", "testDWG2004.dwg",
        "testDWG2004_no_header.dwg", "testDWG2007.dwg", "testDWG2010.dwg",
        "testDWG2010_custom_props.dwg", "testDWGmech2004.dwg", "testDWGmech2004DX.dwg",
        "testDWGmech2005.dwg", "testDWGmech2006.dwg", "testDWGmech2007.dwg",
        "testDWGmech2008.dwg", "testDWGmech2009.dwg", "testDWGmech2010.dwg",
        "testDWGmech2011.dwg", "testDWGmech6.dwg",
    ]],
}


PINNED_SHA256 = {
    "acadsharp/sample_AC1009_ascii.dxf":
        "d0039feca2eda1ead9a7135854c62e0818d0ae0af842a44e1645cf8c8523fd75",
    "acadsharp/sample_AC1009_binary.dxf":
        "dc8612a2670d8bbb0697d6a4e51b197b6bf2055c08c95b8c646515bb63ca947f",
    "acadsharp/sample_AC1014.dwg":
        "d1ec4519be14ac49ecab5ec024ad951271e00261a1851f623a784a97ba6ede73",
    "acadsharp/sample_AC1015.dwg":
        "8ffbdc713f5838cbbb1596a8422e2b607e5c9e79921e852cbb5abee4af76ea4e",
    "acadsharp/sample_AC1015_ascii.dxf":
        "4f7d7f0d3957d4d32c6c43e4c600eea8e90fc0fcb1b36b11256095a9789a923f",
    "acadsharp/sample_AC1015_binary.dxf":
        "a3b3d56311371355b0fc01c50c2fa8ac741ed87429114fe9fcdfb3d2ad15d535",
    "acadsharp/sample_AC1018.dwg":
        "7d26f908516ec5fb89cc8054af33aff233b68a7b63208bbc5b7f9f4d9bff9866",
    "acadsharp/sample_AC1018_ascii.dxf":
        "71117bafa8f0d9816607768c8fb21676c83f99892c537711a68c24aa7e06e798",
    "acadsharp/sample_AC1018_binary.dxf":
        "d659b8e7964eb30700660eea34454696bbbfc927726c3417d93df8776e88e225",
    "acadsharp/sample_AC1021.dwg":
        "70b5efec9a3c230a9892e810154a760fe72311b1e730fb582f22950135da770a",
    "acadsharp/sample_AC1021_ascii.dxf":
        "39ebdd9a48fef7a6240a04e3b5ca6d45ba5bb30b560fb9dadf26774e1af83731",
    "acadsharp/sample_AC1021_binary.dxf":
        "9650c4f2b44f064e09c8ba10ccf558f9438bf7779bed18e6144887930ddb7cb1",
    "acadsharp/sample_AC1024.dwg":
        "53ecc28e54ff0bbfb022989c47fd1cb16eb07a98825309b66cdf316d900fe4ce",
    "acadsharp/sample_AC1024_ascii.dxf":
        "c97e857047ad4cecd84754ea1a8638e47508e339fd489f1e895ee7332127b372",
    "acadsharp/sample_AC1024_binary.dxf":
        "6a99aed5d669235a12f592d5cce426c6fba29b5462bae8d76f98187a2fdda22f",
    "acadsharp/sample_AC1027.dwg":
        "4c63508dd794d5cdfb56c60aded47928999b02619ea8c2ef435a4b74e60d24d6",
    "acadsharp/sample_AC1027_ascii.dxf":
        "4f26752008643642c37f7bab410b23cfd8b6dd9ee3aaa19746b612ea956b61e6",
    "acadsharp/sample_AC1027_binary.dxf":
        "9a8c4dc15828f62b587273f41cfac2a63ce7614841844a15a15248b49cab7e14",
    "acadsharp/sample_AC1032.dwg":
        "0e8faaca949c9429c92240082d9ea4d3524aca5baf7c7815ec439245c321b528",
    "acadsharp/sample_AC1032_ascii.dxf":
        "c2934f616ecd7097e773b85e5d6e0548d21ccc635abcc8e7276f7d4a585a7ae2",
    "acadsharp/sample_AC1032_binary.dxf":
        "e2c59abecc7bc0fd0ace7b9b6f083d2a16d36077b7a7d7ceccd2aa7edfb348e1",
    "gdal/knot_oob.dgn":
        "bd09d118a595f5afba833c7c61dad83cf02b01f1b224836b8493a164de90da4e",
    "gdal/seed_2d.dgn":
        "dd8465f18569d9289809e9e0962115d365d0a56de021393952a5e7a0a20b527c",
    "gdal/seed_3d.dgn":
        "97c2f00ee6ea96873b7d16e5e898b4850e3d35448299d7d9d37e7d1792b56896",
    "gdal/smalltest.dgn":
        "9d9faddb67216f9d56fc9a1027adc1a927c8c13529dfd3416b771b4dc4e9a284",
    "gdal/test_dgnv8.dgn":
        "8f32f87ce4b16881aa64f5cb9f75c98851833f96fef37ca0ad31aa6bb18d1df0",
    "gdal/test_dgnv8_ref.csv":
        "09f765e0aa8dc06bb1d0da4ed86f060dc0a839d480b3dc67e54841ecceec6433",
    "gdal/test_dgnv8_write_ref.csv":
        "02ed01804dade9188cf5a5363f85c9260241637ade1c3f1dfce1df31d3cce417",
    "safe/TreeTextNodeLabelsWithTags.dgn":
        "8439e6d3a18227122c10c5367799561ebe94737d9d27932f4eb89d743b108270",
    "safe/Water_distribution_mains.dgn":
        "f34bbb1b408b583d9cfc9d280e4d5782559cfa824b4b51210a03cf5c88235b64",
    "tika/architectural_-_annotation_scaling_and_multileaders.dwg":
        "0e74f0aa84b1323c922345f067739c42ee26213dea010b407b87a94212271654",
    "tika/testDWG-AC1027.dwg":
        "c63d82327430e6759d8e6930e5b457afb76538ce3311a2abd15ba12726ac277b",
    "tika/testDWG-AC1032.dwg":
        "2de8f0e86939df1c4c77422deecd474fd958efcf9619554a1bdf83157f9176b2",
    "tika/testDWG2000.dwg":
        "ae8e43ea830d8701646f82ea7185d46dee5bb2834dbbc3ca6417a39803ce3832",
    "tika/testDWG2004.dwg":
        "3f2351f298613703148dcea4a693ee0407f5e7210a5aa901435e6ede450e4973",
    "tika/testDWG2004_no_header.dwg":
        "fc89b40d0b415b648cc3c61a0942081ac3dc3d8c2a998065bf65241514357326",
    "tika/testDWG2007.dwg":
        "758b15d93758b0284ef33389f908c94b23d899a7408a30ea5c5f0843f53bbf1a",
    "tika/testDWG2010.dwg":
        "887781c9e0e151cb9d22738913bcfc35ee13f552c69c0a6c53de7311583dfe2d",
    "tika/testDWG2010_custom_props.dwg":
        "203264e0357e23a2e53061faeba13b035f0462df591252b36f974226a96548db",
    "tika/testDWGmech2004.dwg":
        "14cc9e93ca2c65a98be1472126c283e77072735e9561151c1a92eb541bda976b",
    "tika/testDWGmech2004DX.dwg":
        "afb2aa0e144d7a55a9657dafc82809456190ae44310bf3d2716c456605ef28d3",
    "tika/testDWGmech2005.dwg":
        "441de2d09dd6e96acb48e558a56a374a558b7c34d1388de6d4bc5f0d1a38f488",
    "tika/testDWGmech2006.dwg":
        "f6ce9abe9f01929a553c667fb2c9db55842bd4ed1d99c6ac36f1c0f6c128d33c",
    "tika/testDWGmech2007.dwg":
        "f0f1dda71cc9abbf7d339e8b40827f2c9b2abc118b6c48d243f9ab829254b2de",
    "tika/testDWGmech2008.dwg":
        "9edf4ec9cb9ae9d54b9bf6a826a56ff5779a11aaee188766614079f7dd5c4cd9",
    "tika/testDWGmech2009.dwg":
        "03739d11b602854b7ff72476a085b2e328179aad9ee9e229acf935915c31bade",
    "tika/testDWGmech2010.dwg":
        "b588a667d6474708fcf45ed89c428bf87287c765f8ac5025f59ebd20c37630a4",
    "tika/testDWGmech2011.dwg":
        "86468cab589121abe4fb147471fd0e1ac1134cfb015377cd8f2940c80da8bb08",
    "tika/testDWGmech6.dwg":
        "1a506742e6c8e4f574103733a9ae230bef73ab837148587a46a09776b9009269",
}


ATTEMPTS = 4
TIMEOUT_SECONDS = 60


def sha256_of(path: str) -> str:
    digest = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def fetch(url: str, dest: str, expected: str, key: str) -> None:
    """Downloads to a temp file, verifies the pinned SHA-256, then renames atomically."""
    tmp = dest + ".part"
    last: Exception | None = None
    for attempt in range(1, ATTEMPTS + 1):
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "cadkit-corpus"})
            with urllib.request.urlopen(req, timeout=TIMEOUT_SECONDS) as resp, open(tmp, "wb") as out:
                while chunk := resp.read(1 << 20):
                    out.write(chunk)
            actual = sha256_of(tmp)
            if actual != expected:
                # Not retried: a changed upstream file must be reviewed, not silently accepted.
                os.remove(tmp)
                sys.exit(
                    f"error: SHA-256 mismatch for {key}\n"
                    f"  expected {expected}\n"
                    f"  got      {actual}\n"
                    f"  url      {url}\n"
                    "The upstream file changed or the download is corrupt. If the change is "
                    "intended, update PINNED_SHA256 in scripts/fetch-corpus.py."
                )
            os.replace(tmp, dest)
            return
        except (urllib.error.URLError, TimeoutError, OSError) as e:
            last = e
            if os.path.exists(tmp):
                os.remove(tmp)
            if attempt < ATTEMPTS:
                wait = 2 ** attempt
                print(f"  retry {attempt}/{ATTEMPTS - 1} for {key} in {wait}s: {e}")
                time.sleep(wait)
    sys.exit(f"error: could not download {key} from {url} after {ATTEMPTS} attempts: {last}")


def main() -> int:
    manifest = {}
    for source, urls in FILES.items():
        folder = os.path.join(ROOT, source)
        os.makedirs(folder, exist_ok=True)
        for entry in urls:
            url, name = entry if isinstance(entry, tuple) else (entry, entry.rsplit("/", 1)[-1])
            key = f"{source}/{name}"
            expected = PINNED_SHA256.get(key)
            if expected is None:
                sys.exit(f"error: no pinned SHA-256 for {key}; add it to PINNED_SHA256")
            dest = os.path.join(folder, name)
            if os.path.exists(dest) and sha256_of(dest) != expected:
                print(f"discard {key}: hash differs from the pin")
                os.remove(dest)
            if not os.path.exists(dest):
                print(f"fetch {key}")
                fetch(url, dest, expected, key)
            manifest[key] = {"url": url, "sha256": expected}
    with open(os.path.join(ROOT, "MANIFEST.json"), "w", encoding="utf-8") as f:
        json.dump(manifest, f, indent=2)
    print(f"{len(manifest)} files in {os.path.normpath(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
