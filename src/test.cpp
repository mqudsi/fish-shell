#include "test.h"
#include <memory>

std::unique_ptr<vec_results_t> get_result() {
    std::vector<std::string> results;
    results.push_back("hello");
    results.push_back("world");

    return std::unique_ptr<vec_results_t>(new vec_results_t { std::move(results) });
}

const std::vector<std::string> &vec_results_t::get_vec() const {
    return this->results;
}

std::unique_ptr<std::vector<std::string>> vec_results_t::get_vec2() const {
    auto vec = std::unique_ptr<std::vector<std::string>>(new std::vector<std::string>{});
    vec->push_back("hello");
    vec->push_back("world");
    return vec;
}
