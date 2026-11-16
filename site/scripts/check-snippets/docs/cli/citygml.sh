cadkit convert surfaces.dxf model.gml --crs urn:ogc:def:crs:EPSG::32635 --lod 1
cadkit validate-gml model.gml > validation.json
cadkit validate-gml model.gml --tkgm-profile city-model-tender > preflight.json \
  || echo "preflight reported issues; see preflight.json"
