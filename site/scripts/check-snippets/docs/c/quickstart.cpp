#include <fstream>
#include <iostream>
#include "cadkit.hpp"

int main() {
    try {
        auto doc = cadkit::Document::read_file("plan.dxf");
        std::cout << doc.info().entity_count << " entities\n";
        cadkit::SvgOptions svg;
        svg.width_px = 1200;
        std::ofstream("plan.svg") << doc.to_svg(svg);
        std::ofstream("plan-r2000.dxf") << doc.to_dxf(CADKIT_DXF_R2000);
    } catch (const cadkit::Error& e) {
        std::cerr << e.what() << " (status " << static_cast<int>(e.status()) << ")\n";
        return 1;
    }
}
