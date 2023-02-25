#include <vector>
#include <memory>
#include <string>

struct vec_results_t {
    std::vector<std::string> results;

    vec_results_t(std::vector<std::string> &&r) : results(r) {}

    const std::vector<std::string> &get_vec() const;
    std::unique_ptr<std::vector<std::string>> get_vec2() const;
};

std::unique_ptr<vec_results_t> get_result();
