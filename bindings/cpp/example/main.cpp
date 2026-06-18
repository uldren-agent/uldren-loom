// Minimal example exercising the Uldren Loom C++ wrapper.
// Build: see bindings/cpp/README.md (links against libuldren_loom from loom-ffi).
#include <iostream>
#include <vector>

#include "loom.hpp"

int main() {
    std::cout << "loom " << uldren::loom::version() << "\n";
    std::vector<std::uint8_t> abc = {'a', 'b', 'c'};
    std::cout << uldren::loom::blob_digest(abc) << "\n";

    try {
        uldren::loom::sql db("example.loom", "app", "main");
        db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
        db.exec("INSERT INTO t VALUES (1, 'hello')");
        // Typed result: walk the first statement's rows and read cells as faithful values.
        uldren::loom::result r = db.exec("SELECT id, v FROM t");
        for (std::size_t row = 0; row < r.row_count(0); ++row) {
            std::cout << r.cell(0, row, 0).as_int64() << " " << r.cell(0, row, 1).text() << "\n";
        }
        // The JSON debug form is still available.
        std::cout << db.exec_json("SELECT id, v FROM t") << "\n";
        std::cout << "commit " << db.commit("seed", "example") << "\n";
    } catch (const uldren::loom::error &e) {
        std::cerr << "loom error " << e.code << ": " << e.what() << "\n";
        return 1;
    }
    return 0;
}
