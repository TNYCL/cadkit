// cadkit C++17 wrapper: header-only RAII layer over cadkit.h.
// License: MIT OR Apache-2.0.
//
//   cadkit::Document doc = cadkit::Document::read_file("plan.dxf");
//   std::string svg = doc.to_svg();
//
// Errors are reported as cadkit::Error (derives std::runtime_error) carrying the status code
// and the library's message. Document is move-only and frees its handle on destruction.
#ifndef CADKIT_HPP
#define CADKIT_HPP

#include <cstddef>
#include <cstdint>
#include <memory>
#include <optional>
#include <stdexcept>
#include <string>
#include <utility>
#include <vector>

#include "cadkit.h"

namespace cadkit {

/// Exception thrown when a cadkit call fails.
class Error : public std::runtime_error {
public:
    Error(cadkit_status status, const std::string& message)
        : std::runtime_error(message.empty() ? std::string(cadkit_status_string(status)) : message),
          status_(status) {}

    /// The C status code.
    cadkit_status status() const noexcept { return status_; }

private:
    cadkit_status status_;
};

/// Library version, e.g. "0.1.0".
inline std::string version() { return cadkit_version(); }

/// Detects the format from the header only.
inline cadkit_format detect(const std::uint8_t* data, std::size_t len) noexcept {
    return cadkit_detect(data, len);
}
inline cadkit_format detect(const std::vector<std::uint8_t>& bytes) noexcept {
    return cadkit_detect(bytes.data(), bytes.size());
}

/// Read options; zero fields use library defaults.
struct ReadOptions {
    std::uint64_t max_input_bytes = 0;
    std::uint64_t max_decompressed_bytes = 0;
    std::uint64_t max_objects = 0;
    std::uint32_t max_depth = 0;
    std::uint32_t max_string_bytes = 0;
    std::uint32_t max_vertices = 0;
    bool keep_raw = false;
    std::string fallback_codepage;  // empty = none
};

/// SVG rendering options.
struct SvgOptions {
    std::uint32_t model_index = 0;
    double width_px = 1600.0;
    double stroke_px = 1.0;
    std::optional<std::string> background = std::string("#ffffff");  // nullopt = transparent
    bool text = true;
    bool expand_blocks = true;
};

namespace detail {

inline void check(cadkit_status status) {
    if (status != CADKIT_OK) {
        throw Error(status, cadkit_last_error_message());
    }
}

struct StringDeleter {
    void operator()(char* s) const noexcept { cadkit_string_free(s); }
};
using OwnedString = std::unique_ptr<char, StringDeleter>;

inline std::string take(char* raw) {
    OwnedString owned(raw);
    return owned ? std::string(owned.get()) : std::string();
}

}  // namespace detail

/// Move-only owner of a cadkit_document.
class Document {
public:
    Document() noexcept = default;
    Document(const Document&) = delete;
    Document& operator=(const Document&) = delete;
    Document(Document&& other) noexcept : handle_(std::exchange(other.handle_, nullptr)) {}
    Document& operator=(Document&& other) noexcept {
        if (this != &other) {
            reset();
            handle_ = std::exchange(other.handle_, nullptr);
        }
        return *this;
    }
    ~Document() { reset(); }

    /// Reads a drawing file (UTF-8 path). Throws cadkit::Error.
    static Document read_file(const std::string& utf8_path, const ReadOptions& options = {}) {
        cadkit_options c = convert(options);
        cadkit_document* out = nullptr;
        detail::check(cadkit_read_file(utf8_path.c_str(), &c, &out));
        return Document(out);
    }

    /// Reads a drawing from memory. Throws cadkit::Error.
    static Document read_bytes(const std::uint8_t* data, std::size_t len, const ReadOptions& options = {}) {
        cadkit_options c = convert(options);
        cadkit_document* out = nullptr;
        detail::check(cadkit_read_bytes(data, len, &c, &out));
        return Document(out);
    }
    static Document read_bytes(const std::vector<std::uint8_t>& bytes, const ReadOptions& options = {}) {
        return read_bytes(bytes.data(), bytes.size(), options);
    }

    /// True if this object owns a document.
    explicit operator bool() const noexcept { return handle_ != nullptr; }
    const cadkit_document* get() const noexcept { return handle_; }

    /// Summary counts. Throws cadkit::Error.
    cadkit_info info() const {
        cadkit_info out{};
        detail::check(cadkit_document_info(require(), &out));
        return out;
    }

    /// Summary as a JSON object.
    std::string info_json() const {
        char* out = nullptr;
        detail::check(cadkit_document_info_json(require(), &out));
        return detail::take(out);
    }

    /// Full model as JSON.
    std::string to_json(bool pretty = false) const {
        char* out = nullptr;
        detail::check(cadkit_document_to_json(require(), pretty ? 1 : 0, &out));
        return detail::take(out);
    }

    /// Renders one model as SVG.
    std::string to_svg(const SvgOptions& options = {}) const {
        cadkit_svg_options c;
        cadkit_svg_options_init(&c);
        c.model_index = options.model_index;
        c.width_px = options.width_px;
        c.stroke_px = options.stroke_px;
        c.background = options.background ? options.background->c_str() : nullptr;
        c.text = options.text ? 1 : 0;
        c.expand_blocks = options.expand_blocks ? 1 : 0;
        char* out = nullptr;
        detail::check(cadkit_document_to_svg(require(), &c, &out));
        return detail::take(out);
    }

    /// Writes ASCII DXF.
    std::string to_dxf(cadkit_dxf_version version = CADKIT_DXF_R2018) const {
        char* out = nullptr;
        detail::check(cadkit_document_to_dxf(require(), version, &out));
        return detail::take(out);
    }

private:
    explicit Document(cadkit_document* handle) noexcept : handle_(handle) {}

    const cadkit_document* require() const {
        if (handle_ == nullptr) {
            throw Error(CADKIT_NULL_POINTER, "document is empty (moved-from or default-constructed)");
        }
        return handle_;
    }

    void reset() noexcept {
        if (handle_ != nullptr) {
            cadkit_document_free(handle_);
            handle_ = nullptr;
        }
    }

    static cadkit_options convert(const ReadOptions& o) {
        cadkit_options c;
        cadkit_options_init(&c);
        c.max_input_bytes = o.max_input_bytes;
        c.max_decompressed_bytes = o.max_decompressed_bytes;
        c.max_objects = o.max_objects;
        c.max_depth = o.max_depth;
        c.max_string_bytes = o.max_string_bytes;
        c.max_vertices = o.max_vertices;
        c.keep_raw = o.keep_raw ? 1 : 0;
        // The pointer is only read during the read call, while `o` is alive.
        c.fallback_codepage = o.fallback_codepage.empty() ? nullptr : o.fallback_codepage.c_str();
        return c;
    }

    cadkit_document* handle_ = nullptr;
};

}  // namespace cadkit

#endif  // CADKIT_HPP
