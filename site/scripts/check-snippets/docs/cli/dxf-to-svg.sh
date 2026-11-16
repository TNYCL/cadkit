cadkit convert plan.dxf plan.svg --width 1600
cadkit convert plan.dxf walls.svg --width 1600 --layer WALLS      # one layer
cadkit convert plan.dxf layout.svg --model 1                       # a paper layout
cadkit convert - plan.svg --explode-inserts --units mm < plan.dxf  # from stdin
