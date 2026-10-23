// Exercise ownership and immutable operations through the actual C++17 wrapper.
#include "cadkit.hpp"

#include <cstring>

int main() {
    const auto require = [](bool ok) { if (!ok) throw std::runtime_error("C++ contract failed"); };
    const char* dxf = "0\nSECTION\n2\nHEADER\n9\n$INSUNITS\n70\n4\n0\nENDSEC\n0\nSECTION\n2\nENTITIES\n0\nLINE\n10\n0\n20\n0\n11\n1000\n21\n0\n0\nENDSEC\n0\nEOF\n";
    std::vector<std::uint8_t> bytes(dxf, dxf + std::strlen(dxf));
    require(cadkit::Document::inspect_bytes(bytes).find("millimeter") != std::string::npos);
    auto original = cadkit::Document::read_bytes(bytes);
    const auto json = original.to_json();
    auto meters = original.convert_units("meter");
    require(meters.to_json().find("meter") != std::string::npos);
    require(original.to_json() == json);
    auto expanded = meters.explode_inserts();
    auto encoded = expanded.to_dxf_bytes(CADKIT_DXF_R2000, "DOS857");
    require(cadkit::Document::read_bytes(encoded).info().entity_count == 1);
    try { original.convert_units("bad-unit"); require(false); }
    catch (const cadkit::Error& e) { require(e.status() == CADKIT_INVALID_ARGUMENT); }
}
