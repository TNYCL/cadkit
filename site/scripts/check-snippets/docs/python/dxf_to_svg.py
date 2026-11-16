import cadkit

doc = cadkit.read("plan.dxf")
svg = doc.to_svg(
    model=0,           # model space; paper layouts follow
    width=1600,        # pixels; the height follows the drawing's aspect ratio
    stroke=1.0,        # stroke width in pixels
    background=None,   # transparent; or a CSS colour such as "#ffffff"
    text=True,
    include_hidden_layers=False,
)
with open("plan.svg", "w", encoding="utf-8") as f:
    f.write(svg)
