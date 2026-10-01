#!/usr/bin/env python3
"""CityGML 2.0 dosyalarını önbellekteki resmî OGC şemalarıyla denetler."""

import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.parse
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
BASE = "https://schemas.opengis.net/"
MODULES = ["citygml/2.0/cityGMLBase.xsd", "citygml/building/2.0/building.xsd",
           "citygml/generics/2.0/generics.xsd", "citygml/cityobjectgroup/2.0/cityObjectGroup.xsd"]
XS = "{http://www.w3.org/2001/XMLSchema}"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("files", nargs="*")
    parser.add_argument("--fetch", action="store_true", help="Download missing public schemas")
    args = parser.parse_args()
    cache = ROOT / "corpus/public/citygml-schemas"
    cache.mkdir(parents=True, exist_ok=True)
    todo = [BASE + module for module in MODULES]
    seen = {}
    while todo:
        url = todo.pop()
        if url in seen:
            continue
        if len(seen) >= 256:
            raise RuntimeError("schema dependency limit")
        target = cache / (hashlib.sha256(url.encode()).hexdigest() + ".xsd")
        download = url.replace("http://", "https://", 1)
        if "docs.oasis-open.org/election/external/xAL.xsd" in url:
            download = BASE + "citygml/xAL/xAL.xsd"
        if urllib.parse.urlparse(download).hostname not in {"schemas.opengis.net", "www.w3.org"}:
            raise RuntimeError("unexpected schema host")
        if not target.exists():
            if not args.fetch:
                raise RuntimeError("missing schema cache; run with --fetch")
            data = subprocess.run(["curl", "--fail", "--silent", "--show-error", "--location", "--max-time", "30", "--max-filesize", "4194304", download], check=True, capture_output=True).stdout
            if len(data) > 4 * 1024 * 1024:
                raise RuntimeError("schema size limit")
            ET.fromstring(data)
            target.write_bytes(data)
        data = target.read_bytes()
        seen[url] = (target, hashlib.sha256(data).hexdigest(), download)
        schema = ET.fromstring(data)
        for item in schema:
            location = item.get("schemaLocation")
            if item.tag in {XS + "import", XS + "include", XS + "redefine"} and location:
                todo.append(urllib.parse.urljoin(url, location))
    catalog = ET.Element("catalog", xmlns="urn:oasis:names:tc:entity:xmlns:xml:catalog")
    for url, (path, _, _) in seen.items():
        for variant in {url, url.replace("https://", "http://", 1), url.replace("http://", "https://", 1)}:
            ET.SubElement(catalog, "uri", name=variant, uri=path.as_uri())
            ET.SubElement(catalog, "system", systemId=variant, uri=path.as_uri())
    # Şemaların namespace bildirimleri korunur; dosyaları yeniden serileştirmek yerine katalog kullanılır.
    for url, (path, _, _) in seen.items():
        schema = ET.parse(path).getroot()
        for item in schema:
            location = item.get("schemaLocation")
            if location and not urllib.parse.urlparse(location).scheme:
                resolved = urllib.parse.urljoin(url, location)
                local_uri = urllib.parse.urljoin(path.as_uri(), location)
                ET.SubElement(catalog, "uri", name=local_uri, uri=seen[resolved][0].as_uri())
                ET.SubElement(catalog, "system", systemId=local_uri, uri=seen[resolved][0].as_uri())
    catalog_path = cache / "catalog.xml"
    ET.ElementTree(catalog).write(catalog_path, encoding="utf-8", xml_declaration=True)
    driver = ET.Element(XS + "schema")
    for module in MODULES:
        url = BASE + module
        namespace = ET.parse(seen[url][0]).getroot().get("targetNamespace")
        ET.SubElement(driver, XS + "import", namespace=namespace, schemaLocation=url)
    driver_path = cache / "profile.xsd"
    ET.ElementTree(driver).write(driver_path, encoding="utf-8", xml_declaration=True)
    (cache / "manifest.json").write_text(json.dumps({url: {"sha256": sha, "download": download} for url, (_, sha, download) in seen.items()}, indent=2))
    import os
    environment = dict(os.environ, XML_CATALOG_FILES=str(catalog_path))
    for name in args.files:
        subprocess.run(["xmllint", "--nonet", "--noout", "--schema", str(driver_path), name], env=environment, check=True)
    print(f"Official schema cache ready: {len(seen)} schemas")


if __name__ == "__main__":
    main()
