cadkit strip-seed template.dgn seed.dgn --model-name Model --report > seed-report.json
cadkit layers seed.dgn                         # review the level names the seed keeps
cadkit convert drawing.dgn converted.dgn --seed seed.dgn
cadkit check-dgn converted.dgn > structure.json  # exits 1 when the report has warnings
