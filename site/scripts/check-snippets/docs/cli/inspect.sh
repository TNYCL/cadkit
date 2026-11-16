cadkit inspect plan.dwg --json            # header only: format, version, units, code page
cadkit info plan.dwg --warnings           # summary plus the first 20 warnings
cadkit layers plan.dgn --json             # DGN levels are layers
cadkit blocks plan.dxf                    # block definitions with entity counts
cadkit dump plan.dxf --kind arc           # one line per entity of a kind
