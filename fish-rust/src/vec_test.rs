use cxx::{CxxVector, CxxString};

#[cxx::bridge]
mod ffi {
    unsafe extern "C++" {
        include!("test.h");

        type vec_results_t;

        fn get_result() -> UniquePtr<vec_results_t>;
        fn get_vec(&self) -> &CxxVector<CxxString>;
        fn get_vec2(&self) -> UniquePtr<CxxVector<CxxString>>;
    }
}

fn test_vec() {
    let r = self::ffi::get_result();
    let results: &CxxVector<CxxString> = r.get_vec();

    println!("There are {} results", results.len());
    for r in results.iter() {
        println!("{}", r);
    }
}

fn test_vec2() {
    let r = self::ffi::get_result();
    let results = r.get_vec2();

    println!("There are {} results", results.len());
    for r in results.iter() {
        println!("{}", r);
    }
}
