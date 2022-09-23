#include <functional>
#include <memory>
#include <new>
#include <string>

/// A copy-on-write container that contains either an object T or a reference to an object T.
template<typename T>
struct cow_t {
private:
    union __inner {
        std::reference_wrapper<const T> ref;
        T value;

        __inner() {
        }

        __inner(const T &r) : ref(r) {
        }

        __inner(T &&v) : value(v) {
        }

        ~__inner() {
            // value's destructor must be manually called!
        }
    } inner_;

    enum class state_t : uint8_t {
        none,
        owned,
        ref,
    } type_;

    void reset() {
        if (type_ == state_t::owned) {
            inner_.value.~T();
        }
        type_ = state_t::none;
    }

    /// Creates a cow_t instance containing a reference to the data in \param other.
    explicit cow_t(const T &ref) : inner_(ref) {
        type_ = state_t::ref;
    }

public:

    // template<typename U = T, typename std::enable_if<!std::is_reference<U>::value>::type* = nullptr>
    cow_t(T &&value) noexcept : inner_(std::move(value)) {
        type_ = state_t::owned;
    }

    /// Creates a new cow_t referencing the data in \param other.
    cow_t(const cow_t &other) : inner_() {
        // It's not safe to always create a reference to the existing value as it may have been a
        // temporary and we have no way to tell.
        if (other.type_ == state_t::owned) {
            type_ = state_t::owned;
            new (&inner_.value) T(other.value());
        } else if (other.type_ == state_t::ref) {
            type_ = state_t::ref;
            new (&inner_.ref) std::reference_wrapper<const T>(other.value());
        } else {
            assert(false && "cow_t copying from deleted or moved cow!");
        }
    }

    /// Moves the data in \param other into a new cow_t instance.
    cow_t(cow_t &&other) noexcept : inner_() {
        if (other.type_ == state_t::owned) {
            new (&inner_.value) T(std::move(other.inner_.value));
        } else if (other.type_ == state_t::ref) {
            new (&inner_.ref) std::reference_wrapper<const T>(std::move(other.inner_.ref));
        } else {
            assert(false && "moving into new cow_t from deleted cow_t!");
        }
        type_ = other.type_;
        other.type_ = state_t::none;
    }

    /// Creates a cow_t instance containing a copy of the data in \param other.
    static cow_t clone(const T &other) {
        return std::move(T(other));
    }

    /// Creates a cow_t instance containing a reference to the data in \param other.
    static cow_t ref(const T &other) {
        return cow_t(other);
    }

    /// Construct a value in-place.
    template <class... Args>
    void emplace(Args &&...args) {
        type_ = state_t::owned;
        new (&inner_.value) T(std::forward<Args>(args)...);
    }

    /// Destructs any data owned by the cow_t.
    ~cow_t() {
        if (type_ == state_t::owned) {
            inner_.value.~T();
        }
        type_ = state_t::none;
    }

    /// Returns a constant reference to the underlying data.
    const T &value() const {
        if (type_ == state_t::owned) {
            return inner_.value;
        } else if (type_ == state_t::ref) {
            return inner_.ref;
        } else {
            assert(false && "trying to dereference moved/deleted cow_t!");
        }
    }

    /// Implicitly converts a cow_t to a constant reference to the underlying data.
    operator const T &() const {
        return value();
    }

    /// Returns a copy of the owned or referenced value.
    T to_owned() const {
        if (type_ == state_t::owned) {
            return inner_.value;
        } else if (type_ == state_t::ref) {
            return inner_.ref;
        } else {
            assert(false && "trying to dereference moved/deleted cow_t!");
        }
    }

    /// Moves the value out of the current cow_t instance if it currently owns it, otherwise returns
    /// a copy of the stored reference.
    T &&take() {
        if (type_ == state_t::owned) {
            return std::move(inner_.value);
        } else if (type_ == state_t::ref) {
            return std::move(T(*inner_.ref));
        } else {
            assert(false && "trying to move out of moved/deleted cow_t!");
        }
        type_ = state_t::none;
    }

    /// Dereferences a cow_t to a pointer to its underlying data.
    const T *operator->() const { return &value(); }

    /// Dereferences a cow_t to a reference to underlying data.
    const T &operator*() const { return value(); }

    cow_t &operator=(const cow_t &other) {
        // Protect against self-assignment
        if (this == &other) {
            return *this;
        }

        this->reset();
        new (this) cow_t(other);
        return *this;
    }

    cow_t &operator=(cow_t &&other) noexcept {
        if (this == &other) {
            return *this;
        }

        this->reset();
        new (this) cow_t(other);
        return *this;
    }

    cow_t &operator=(T &&other) noexcept {
        this->reset();
        new (&inner_.value) T(other);
        type_ = state_t::owned;
        return *this;
    }

    bool operator==(const cow_t &rhs) const { return this->value() == rhs.value(); }
    bool operator!=(const cow_t &rhs) const { return !(*this == rhs); }
    bool operator==(const T &rhs) const { return value() == rhs; }
    bool operator!=(const T &rhs) const { return !(*this == rhs); }

    // Some specializations
    template <class = typename std::enable_if<
    std::is_same<cow_t<T>, cow_t<std::wstring>>::value>::type>
    cow_t(const wchar_t *str) : cow_t(std::move(std::wstring(str))) {}
};

using cowstr_t = cow_t<std::string>;
using wcowstr_t = cow_t<std::wstring>;
