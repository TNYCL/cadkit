// Prints a summary of a DWG/DGN/DXF drawing. Usage: info <file> [out.svg]
#include <fstream>
#include <iostream>

#include "cadkit.hpp"

int main(int argc, char** argv) {
    if (argc < 2) {
        std::cerr << "usage: " << argv[0] << " <file> [out.svg]\n";
        return 2;
    }
    std::cout << "cadkit " << cadkit::version() << "\n";
    try {
        cadkit::Document doc = cadkit::Document::read_file(argv[1]);
        cadkit_info info = doc.info();
        std::cout << "models=" << info.model_count << " layers=" << info.layer_count
                  << " blocks=" << info.block_count << " entities=" << info.entity_count
                  << " warnings=" << info.warning_count << "\n"
                  << doc.info_json() << "\n";
        if (argc > 2) {
            std::ofstream(argv[2], std::ios::binary) << doc.to_svg();
            std::cout << "wrote " << argv[2] << "\n";
        }
    } catch (const cadkit::Error& e) {
        std::cerr << "cadkit error " << static_cast<int>(e.status()) << ": " << e.what() << "\n";
        return 1;
    }
    return 0;
}
