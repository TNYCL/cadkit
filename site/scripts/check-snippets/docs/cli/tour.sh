cadkit --version
cadkit inspect plan.dxf                      # header only
cadkit info plan.dxf                         # models, layers, entity histogram
cadkit layers plan.dxf                       # state and entity count per layer
cadkit convert plan.dxf plan.svg --width 1200
cadkit convert plan.dxf plan.json --pretty
cadkit convert plan.dxf plan-r12.dxf --dxf-version r12
cadkit dump plan.dxf --layer WALLS           # one line per entity
