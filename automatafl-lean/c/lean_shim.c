/*
 * automatafl-lean/c/lean_shim.c
 *
 * Self-contained Lean 4 runtime shim + FFI entrypoints for the compiled
 * Automatafl Lean 4 core (`ListAux.c`, `Base.c`, `Rules.c`, `Termination.c`,
 * `Variants.c`, `PackedBoard.c`, `FFI.c`).
 *
 * Compiles identically on native targets (macOS, Linux, Windows, iOS, Android)
 * and `wasm32-unknown-unknown` without requiring external `libgmp`, `libuv`,
 * or `libc++` archives. Memory allocation delegates to Rust's global allocator
 * via `automatafl_rt_alloc` / `automatafl_rt_free`.
 */

#include <lean/lean.h>

/* ==========================================================================
 * §1. Rust-Backed Memory Primitives (Freestanding-Safe)
 * ========================================================================== */

extern void* automatafl_rt_alloc(size_t sz);
extern void  automatafl_rt_free(void* ptr);
extern void  automatafl_rt_mark_persistent(void);
extern LEAN_NORETURN void automatafl_rt_panic(const char* msg);

static inline void rt_memcpy(void* dst, const void* src, size_t n) {
    uint8_t* d = (uint8_t*)dst;
    const uint8_t* s = (const uint8_t*)src;
    for (size_t i = 0; i < n; i++) {
        d[i] = s[i];
    }
}

static inline int rt_memcmp(const void* a, const void* b, size_t n) {
    const uint8_t* pa = (const uint8_t*)a;
    const uint8_t* pb = (const uint8_t*)b;
    for (size_t i = 0; i < n; i++) {
        if (pa[i] != pb[i]) {
            return (int)pa[i] - (int)pb[i];
        }
    }
    return 0;
}

static inline size_t rt_strlen(const char* s) {
    size_t n = 0;
    while (s[n] != '\0') {
        n++;
    }
    return n;
}

LEAN_EXPORT void* mi_malloc_small(size_t sz) {
    return automatafl_rt_alloc(sz);
}

LEAN_EXPORT lean_object* lean_alloc_object(size_t sz) {
    return (lean_object*)automatafl_rt_alloc(sz);
}

LEAN_EXPORT void lean_free_object(lean_object* o) {
    automatafl_rt_free((void*)o);
}

LEAN_EXPORT void lean_inc_heartbeat(void) {}

LEAN_EXPORT void lean_notify_assert(const char* fileName, int line, const char* condition) {
    (void)fileName;
    (void)line;
    automatafl_rt_panic(condition);
}

LEAN_EXPORT LEAN_NORETURN void lean_internal_panic(const char* msg) {
    automatafl_rt_panic(msg);
}

LEAN_EXPORT LEAN_NORETURN void lean_internal_panic_out_of_memory(void) {
    automatafl_rt_panic("Lean runtime out of memory");
}

LEAN_EXPORT LEAN_NORETURN void lean_internal_panic_unreachable(void) {
    automatafl_rt_panic("Lean runtime unreachable");
}

LEAN_EXPORT LEAN_NORETURN void lean_internal_panic_rc_overflow(void) {
    automatafl_rt_panic("Lean runtime RC overflow");
}

LEAN_EXPORT LEAN_NORETURN void lean_internal_panic_overflow(void) {
    automatafl_rt_panic("Lean runtime overflow");
}

/* ==========================================================================
 * §2. Reference Counting, Persistence & Once Cells
 * ========================================================================== */

LEAN_EXPORT void lean_dec_ref_cold(lean_object* o) {
    if (o == NULL || lean_is_scalar(o)) return;
    if (__atomic_load_n(&o->m_rc, __ATOMIC_ACQUIRE) < 0) return;
    uint8_t tag = o->m_tag;
    if (tag <= LeanMaxCtorTag) {
        unsigned n = lean_ctor_num_objs(o);
        lean_object** objs = lean_ctor_obj_cptr(o);
        for (unsigned i = 0; i < n; i++) {
            lean_dec(objs[i]);
        }
        lean_free_object(o);
    } else if (tag == LeanClosure) {
        lean_closure_object* c = lean_to_closure(o);
        for (unsigned i = 0; i < c->m_num_fixed; i++) {
            lean_dec(c->m_objs[i]);
        }
        lean_free_object(o);
    } else if (tag == LeanArray) {
        lean_array_object* a = lean_to_array(o);
        for (size_t i = 0; i < a->m_size; i++) {
            lean_dec(a->m_data[i]);
        }
        lean_free_object(o);
    } else if (tag == LeanScalarArray || tag == LeanString || tag == LeanMPZ) {
        lean_free_object(o);
    } else if (tag == LeanRef) {
        lean_ref_object* r = lean_to_ref(o);
        lean_dec(r->m_value);
        lean_free_object(o);
    } else if (tag == LeanThunk) {
        lean_thunk_object* t = lean_to_thunk(o);
        if (t->m_value != NULL) lean_dec(t->m_value);
        if (t->m_closure != NULL) lean_dec(t->m_closure);
        lean_free_object(o);
    } else {
        lean_free_object(o);
    }
}

LEAN_EXPORT void lean_mark_persistent(lean_object* o) {
    if (o == NULL || lean_is_scalar(o)) return;
    if (__atomic_load_n(&o->m_rc, __ATOMIC_ACQUIRE) <= 0) return;
    if (__atomic_exchange_n(&o->m_rc, 0, __ATOMIC_ACQ_REL) <= 0) return;
    automatafl_rt_mark_persistent();
    uint8_t tag = o->m_tag;
    if (tag <= LeanMaxCtorTag) {
        unsigned n = lean_ctor_num_objs(o);
        lean_object** objs = lean_ctor_obj_cptr(o);
        for (unsigned i = 0; i < n; i++) {
            lean_mark_persistent(objs[i]);
        }
    } else if (tag == LeanClosure) {
        lean_closure_object* c = lean_to_closure(o);
        for (unsigned i = 0; i < c->m_num_fixed; i++) {
            lean_mark_persistent(c->m_objs[i]);
        }
    } else if (tag == LeanArray) {
        lean_array_object* a = lean_to_array(o);
        for (size_t i = 0; i < a->m_size; i++) {
            lean_mark_persistent(a->m_data[i]);
        }
    } else if (tag == LeanRef) {
        lean_ref_object* r = lean_to_ref(o);
        lean_mark_persistent(r->m_value);
    } else if (tag == LeanThunk) {
        lean_thunk_object* t = lean_to_thunk(o);
        if (t->m_value != NULL) lean_mark_persistent(t->m_value);
        if (t->m_closure != NULL) lean_mark_persistent(t->m_closure);
    }
}

static inline void once_cell_lock(lean_once_cell_t* tok) {
    int expected = 0;
    while (!atomic_compare_exchange_weak_explicit(
        &tok->lock, &expected, 1, memory_order_acquire, memory_order_relaxed)) {
        expected = 0;
    }
}

static inline void once_cell_unlock(lean_once_cell_t* tok) {
    atomic_store_explicit(&tok->lock, 0, memory_order_release);
}

LEAN_EXPORT lean_object* lean_obj_once_cold(lean_object** loc, lean_once_cell_t* tok, lean_object* (*init)(void)) {
    once_cell_lock(tok);
    if (atomic_load_explicit(&tok->state, memory_order_relaxed) == 0) {
        lean_object* v = init();
        lean_mark_persistent(v);
        *loc = v;
        atomic_store_explicit(&tok->state, 1, memory_order_release);
    }
    once_cell_unlock(tok);
    return *loc;
}

LEAN_EXPORT uint8_t lean_uint8_once_cold(uint8_t* loc, lean_once_cell_t* tok, uint8_t (*init)(void)) {
    once_cell_lock(tok);
    if (atomic_load_explicit(&tok->state, memory_order_relaxed) == 0) {
        *loc = init();
        atomic_store_explicit(&tok->state, 1, memory_order_release);
    }
    once_cell_unlock(tok);
    return *loc;
}

LEAN_EXPORT uint16_t lean_uint16_once_cold(uint16_t* loc, lean_once_cell_t* tok, uint16_t (*init)(void)) {
    once_cell_lock(tok);
    if (atomic_load_explicit(&tok->state, memory_order_relaxed) == 0) {
        *loc = init();
        atomic_store_explicit(&tok->state, 1, memory_order_release);
    }
    once_cell_unlock(tok);
    return *loc;
}

LEAN_EXPORT uint32_t lean_uint32_once_cold(uint32_t* loc, lean_once_cell_t* tok, uint32_t (*init)(void)) {
    once_cell_lock(tok);
    if (atomic_load_explicit(&tok->state, memory_order_relaxed) == 0) {
        *loc = init();
        atomic_store_explicit(&tok->state, 1, memory_order_release);
    }
    once_cell_unlock(tok);
    return *loc;
}

LEAN_EXPORT uint64_t lean_uint64_once_cold(uint64_t* loc, lean_once_cell_t* tok, uint64_t (*init)(void)) {
    once_cell_lock(tok);
    if (atomic_load_explicit(&tok->state, memory_order_relaxed) == 0) {
        *loc = init();
        atomic_store_explicit(&tok->state, 1, memory_order_release);
    }
    once_cell_unlock(tok);
    return *loc;
}

LEAN_EXPORT size_t lean_usize_once_cold(size_t* loc, lean_once_cell_t* tok, size_t (*init)(void)) {
    once_cell_lock(tok);
    if (atomic_load_explicit(&tok->state, memory_order_relaxed) == 0) {
        *loc = init();
        atomic_store_explicit(&tok->state, 1, memory_order_release);
    }
    once_cell_unlock(tok);
    return *loc;
}

/* ==========================================================================
 * §3. Closure Application (`lean_apply_1..4`)
 * ========================================================================== */

static lean_object* invoke_closure_saturated(lean_closure_object* c, unsigned total_args, lean_object** all_args) {
    void* fn = c->m_fun;
    switch (total_args) {
        case 1: return ((lean_object*(*)(lean_object*))fn)(all_args[0]);
        case 2: return ((lean_object*(*)(lean_object*, lean_object*))fn)(all_args[0], all_args[1]);
        case 3: return ((lean_object*(*)(lean_object*, lean_object*, lean_object*))fn)(all_args[0], all_args[1], all_args[2]);
        case 4: return ((lean_object*(*)(lean_object*, lean_object*, lean_object*, lean_object*))fn)(all_args[0], all_args[1], all_args[2], all_args[3]);
        case 5: return ((lean_object*(*)(lean_object*, lean_object*, lean_object*, lean_object*, lean_object*))fn)(all_args[0], all_args[1], all_args[2], all_args[3], all_args[4]);
        case 6: return ((lean_object*(*)(lean_object*, lean_object*, lean_object*, lean_object*, lean_object*, lean_object*))fn)(all_args[0], all_args[1], all_args[2], all_args[3], all_args[4], all_args[5]);
        case 7: return ((lean_object*(*)(lean_object*, lean_object*, lean_object*, lean_object*, lean_object*, lean_object*, lean_object*))fn)(all_args[0], all_args[1], all_args[2], all_args[3], all_args[4], all_args[5], all_args[6]);
        case 8: return ((lean_object*(*)(lean_object*, lean_object*, lean_object*, lean_object*, lean_object*, lean_object*, lean_object*, lean_object*))fn)(all_args[0], all_args[1], all_args[2], all_args[3], all_args[4], all_args[5], all_args[6], all_args[7]);
        default:
            automatafl_rt_panic("Closure arity > 8 unsupported in minimal runtime");
    }
}

LEAN_EXPORT lean_object* lean_apply_n(lean_object* f, unsigned n, lean_object** args) {
    lean_closure_object* c = lean_to_closure(f);
    unsigned num_fixed = c->m_num_fixed;
    unsigned arity = c->m_arity;
    if (num_fixed + n < arity) {
        lean_object* res = lean_alloc_closure(c->m_fun, arity, num_fixed + n);
        lean_closure_object* rc = lean_to_closure(res);
        for (unsigned i = 0; i < num_fixed; i++) {
            lean_inc(c->m_objs[i]);
            rc->m_objs[i] = c->m_objs[i];
        }
        for (unsigned i = 0; i < n; i++) {
            rc->m_objs[num_fixed + i] = args[i];
        }
        lean_dec(f);
        return res;
    } else if (num_fixed + n == arity) {
        lean_object* all_args[16];
        for (unsigned i = 0; i < num_fixed; i++) {
            lean_inc(c->m_objs[i]);
            all_args[i] = c->m_objs[i];
        }
        for (unsigned i = 0; i < n; i++) {
            all_args[num_fixed + i] = args[i];
        }
        lean_object* res = invoke_closure_saturated(c, arity, all_args);
        lean_dec(f);
        return res;
    } else {
        unsigned consume = arity - num_fixed;
        lean_object* all_args[16];
        for (unsigned i = 0; i < num_fixed; i++) {
            lean_inc(c->m_objs[i]);
            all_args[i] = c->m_objs[i];
        }
        for (unsigned i = 0; i < consume; i++) {
            all_args[num_fixed + i] = args[i];
        }
        lean_object* next_f = invoke_closure_saturated(c, arity, all_args);
        lean_dec(f);
        return lean_apply_n(next_f, n - consume, args + consume);
    }
}

LEAN_EXPORT lean_object* lean_apply_1(lean_object* f, lean_object* a1) {
    lean_object* args[1] = { a1 };
    return lean_apply_n(f, 1, args);
}

LEAN_EXPORT lean_object* lean_apply_2(lean_object* f, lean_object* a1, lean_object* a2) {
    lean_object* args[2] = { a1, a2 };
    return lean_apply_n(f, 2, args);
}

LEAN_EXPORT lean_object* lean_apply_3(lean_object* f, lean_object* a1, lean_object* a2, lean_object* a3) {
    lean_object* args[3] = { a1, a2, a3 };
    return lean_apply_n(f, 3, args);
}

LEAN_EXPORT lean_object* lean_apply_4(lean_object* f, lean_object* a1, lean_object* a2, lean_object* a3, lean_object* a4) {
    lean_object* args[4] = { a1, a2, a3, a4 };
    return lean_apply_n(f, 4, args);
}

/* ==========================================================================
 * §4. 64-Bit Big Nat / Int Support (`LeanMPZ`)
 * ========================================================================== */

typedef struct {
    lean_object m_header;
    int8_t      m_sign; /* >=0 for non-negative, <0 for negative */
    uint64_t    m_val;  /* magnitude */
} lean_mpz_small_object;

static lean_object* alloc_mpz_u64(uint64_t v) {
    if (v <= LEAN_MAX_SMALL_NAT) {
        return lean_box((size_t)v);
    }
    lean_mpz_small_object* o = (lean_mpz_small_object*)lean_alloc_object(sizeof(lean_mpz_small_object));
    lean_set_st_header((lean_object*)o, LeanMPZ, 0);
    o->m_sign = 1;
    o->m_val = v;
    return (lean_object*)o;
}

static lean_object* alloc_mpz_i64(int64_t v) {
    if (v >= LEAN_MIN_SMALL_INT && v <= LEAN_MAX_SMALL_INT) {
        return lean_int64_to_int(v);
    }
    lean_mpz_small_object* o = (lean_mpz_small_object*)lean_alloc_object(sizeof(lean_mpz_small_object));
    lean_set_st_header((lean_object*)o, LeanMPZ, 0);
    if (v < 0) {
        o->m_sign = -1;
        o->m_val = (uint64_t)(-(v + 1)) + 1u;
    } else {
        o->m_sign = 1;
        o->m_val = (uint64_t)v;
    }
    return (lean_object*)o;
}

static uint64_t nat_to_u64(b_lean_obj_arg a) {
    if (lean_is_scalar(a)) {
        return (uint64_t)lean_unbox(a);
    }
    return ((lean_mpz_small_object*)a)->m_val;
}

static int64_t int_to_i64(b_lean_obj_arg a) {
    if (lean_is_scalar(a)) {
        return lean_scalar_to_int64(a);
    }
    lean_mpz_small_object* m = (lean_mpz_small_object*)a;
    return (m->m_sign < 0) ? -(int64_t)m->m_val : (int64_t)m->m_val;
}

LEAN_EXPORT lean_obj_res lean_big_usize_to_nat(size_t n) { return alloc_mpz_u64((uint64_t)n); }
LEAN_EXPORT lean_obj_res lean_big_uint64_to_nat(uint64_t n) { return alloc_mpz_u64(n); }
LEAN_EXPORT lean_object* lean_big_size_t_to_int(size_t n) { return alloc_mpz_u64((uint64_t)n); }
LEAN_EXPORT lean_object* lean_big_int64_to_int(int64_t n) { return alloc_mpz_i64(n); }

LEAN_EXPORT uint8_t lean_uint8_of_big_nat(b_lean_obj_arg a) { return (uint8_t)nat_to_u64(a); }
LEAN_EXPORT uint32_t lean_uint32_of_big_nat(b_lean_obj_arg a) { return (uint32_t)nat_to_u64(a); }
LEAN_EXPORT uint64_t lean_uint64_of_big_nat(b_lean_obj_arg a) { return nat_to_u64(a); }

LEAN_EXPORT lean_object* lean_nat_big_add(lean_object* a1, lean_object* a2) { return alloc_mpz_u64(nat_to_u64(a1) + nat_to_u64(a2)); }
LEAN_EXPORT lean_object* lean_nat_big_sub(lean_object* a1, lean_object* a2) {
    uint64_t v1 = nat_to_u64(a1), v2 = nat_to_u64(a2);
    return alloc_mpz_u64(v1 >= v2 ? v1 - v2 : 0);
}
LEAN_EXPORT lean_object* lean_nat_big_mul(lean_object* a1, lean_object* a2) { return alloc_mpz_u64(nat_to_u64(a1) * nat_to_u64(a2)); }
LEAN_EXPORT lean_object* lean_nat_overflow_mul(size_t a1, size_t a2) { return alloc_mpz_u64((uint64_t)a1 * (uint64_t)a2); }
LEAN_EXPORT lean_object* lean_nat_big_div(lean_object* a1, lean_object* a2) {
    uint64_t v2 = nat_to_u64(a2);
    return v2 == 0 ? lean_box(0) : alloc_mpz_u64(nat_to_u64(a1) / v2);
}
LEAN_EXPORT lean_object* lean_nat_big_mod(lean_object* a1, lean_object* a2) {
    uint64_t v1 = nat_to_u64(a1), v2 = nat_to_u64(a2);
    return v2 == 0 ? alloc_mpz_u64(v1) : alloc_mpz_u64(v1 % v2);
}
LEAN_EXPORT bool lean_nat_big_eq(lean_object* a1, lean_object* a2) { return nat_to_u64(a1) == nat_to_u64(a2); }
LEAN_EXPORT bool lean_nat_big_le(lean_object* a1, lean_object* a2) { return nat_to_u64(a1) <= nat_to_u64(a2); }
LEAN_EXPORT bool lean_nat_big_lt(lean_object* a1, lean_object* a2) { return nat_to_u64(a1) < nat_to_u64(a2); }

LEAN_EXPORT lean_object* lean_int_big_neg(lean_object* a) { return alloc_mpz_i64(-int_to_i64(a)); }
LEAN_EXPORT lean_object* lean_int_big_add(lean_object* a1, lean_object* a2) { return alloc_mpz_i64(int_to_i64(a1) + int_to_i64(a2)); }
LEAN_EXPORT bool lean_int_big_eq(lean_object* a1, lean_object* a2) { return int_to_i64(a1) == int_to_i64(a2); }
LEAN_EXPORT bool lean_int_big_le(lean_object* a1, lean_object* a2) { return int_to_i64(a1) <= int_to_i64(a2); }
LEAN_EXPORT bool lean_int_big_lt(lean_object* a1, lean_object* a2) { return int_to_i64(a1) < int_to_i64(a2); }

LEAN_EXPORT lean_object* l_Int_toNat(lean_object* a) {
    int64_t v = int_to_i64(a);
    return v <= 0 ? lean_box(0) : alloc_mpz_u64((uint64_t)v);
}

/* ==========================================================================
 * §5. Arrays & ByteArrays (`lean_array_*`, `lean_byte_array_*`)
 * ========================================================================== */

LEAN_EXPORT lean_object* lean_array_push(lean_obj_arg a, lean_obj_arg v) {
    lean_array_object* arr = lean_to_array(a);
    size_t sz = arr->m_size;
    size_t cap = arr->m_capacity;
    if (lean_is_exclusive(a) && sz < cap) {
        arr->m_data[sz] = v;
        arr->m_size = sz + 1;
        return a;
    }
    size_t new_cap = (cap == 0) ? 4 : (sz < cap ? cap : cap * 2);
    lean_object* res = lean_alloc_array(sz + 1, new_cap);
    lean_array_object* rarr = lean_to_array(res);
    for (size_t i = 0; i < sz; i++) {
        lean_object* elem = arr->m_data[i];
        lean_inc(elem);
        rarr->m_data[i] = elem;
    }
    rarr->m_data[sz] = v;
    lean_dec(a);
    return res;
}

LEAN_EXPORT lean_object* lean_array_to_list(lean_obj_arg a) {
    lean_array_object* arr = lean_to_array(a);
    size_t i = arr->m_size;
    lean_object* r = lean_box(0);
    while (i > 0) {
        i--;
        lean_object* elem = arr->m_data[i];
        lean_inc(elem);
        lean_object* cell = lean_alloc_ctor(1, 2, 0);
        lean_ctor_set(cell, 0, elem);
        lean_ctor_set(cell, 1, r);
        r = cell;
    }
    lean_dec(a);
    return r;
}

static lean_sarray_object g_empty_byte_array = {
    .m_header = { .m_rc = 0, .m_cs_sz = 0, .m_other = 1, .m_tag = LeanScalarArray },
    .m_size = 0,
    .m_capacity = 0
};
LEAN_EXPORT lean_object* l_ByteArray_empty = (lean_object*)&g_empty_byte_array;

LEAN_EXPORT lean_obj_res lean_byte_array_mk(lean_obj_arg a) {
    lean_array_object* arr = lean_to_array(a);
    size_t sz = arr->m_size;
    lean_object* res = lean_alloc_sarray(1, sz, sz);
    uint8_t* dst = lean_sarray_cptr(res);
    for (size_t i = 0; i < sz; i++) {
        dst[i] = (uint8_t)lean_unbox(arr->m_data[i]);
    }
    lean_dec(a);
    return res;
}

LEAN_EXPORT lean_obj_res lean_byte_array_push(lean_obj_arg a, uint8_t b) {
    lean_sarray_object* sarr = lean_to_sarray(a);
    size_t sz = sarr->m_size;
    size_t cap = sarr->m_capacity;
    if (lean_is_exclusive(a) && sz < cap) {
        sarr->m_data[sz] = b;
        sarr->m_size = sz + 1;
        return a;
    }
    size_t new_cap = (cap == 0) ? 128 : (sz < cap ? cap : cap * 2);
    if (new_cap < sz + 1) new_cap = sz + 1;
    lean_object* res = lean_alloc_sarray(1, sz + 1, new_cap);
    uint8_t* dst = lean_sarray_cptr(res);
    if (sz > 0) {
        rt_memcpy(dst, sarr->m_data, sz);
    }
    dst[sz] = b;
    lean_dec(a);
    return res;
}

LEAN_EXPORT lean_object* lean_byte_array_copy_slice(
    b_lean_obj_arg src,
    lean_obj_arg o_src_off,
    lean_obj_arg dest,
    lean_obj_arg o_dest_off,
    lean_obj_arg o_len,
    uint8_t exact
) {
    (void)exact;
    size_t src_sz = lean_sarray_size(src);
    size_t dest_sz = lean_sarray_size(dest);
    size_t src_off = lean_unbox(o_src_off);
    size_t dest_off = lean_unbox(o_dest_off);
    size_t copy_len = lean_unbox(o_len);
    lean_dec(o_src_off);
    lean_dec(o_dest_off);
    lean_dec(o_len);

    if (src_off >= src_sz) {
        copy_len = 0;
    } else if (copy_len > src_sz - src_off) {
        copy_len = src_sz - src_off;
    }
    size_t new_sz = (dest_off + copy_len > dest_sz) ? (dest_off + copy_len) : dest_sz;
    lean_object* out = lean_alloc_sarray(1, new_sz, new_sz);
    uint8_t* out_ptr = lean_sarray_cptr(out);
    for (size_t i = 0; i < new_sz; i++) out_ptr[i] = 0;
    if (dest_sz > 0) {
        rt_memcpy(out_ptr, lean_sarray_cptr(dest), dest_sz);
    }
    if (copy_len > 0) {
        rt_memcpy(out_ptr + dest_off, lean_sarray_cptr(src) + src_off, copy_len);
    }
    lean_dec(dest);
    return out;
}

/* ==========================================================================
 * §6. Strings (`lean_string_*`, `l_String_*`, `l_Nat_reprFast`, `l_Int_repr`)
 * ========================================================================== */

static size_t utf8_count_chars(const char* s, size_t byte_len) {
    size_t count = 0;
    for (size_t i = 0; i < byte_len; i++) {
        if ((((uint8_t)s[i]) & 0xC0) != 0x80) {
            count++;
        }
    }
    return count;
}

static lean_object* mk_string_bytes(const char* s, size_t byte_len) {
    size_t sz = byte_len + 1;
    lean_string_object* o = (lean_string_object*)lean_alloc_object(sizeof(lean_string_object) + sz);
    lean_set_st_header((lean_object*)o, LeanString, 0);
    o->m_size = sz;
    o->m_capacity = sz;
    o->m_length = utf8_count_chars(s, byte_len);
    if (byte_len > 0) {
        rt_memcpy(o->m_data, s, byte_len);
    }
    o->m_data[byte_len] = '\0';
    return (lean_object*)o;
}

LEAN_EXPORT lean_obj_res lean_string_append(lean_obj_arg s1, b_lean_obj_arg s2) {
    size_t len1 = lean_string_size(s1) - 1;
    size_t len2 = lean_string_size(s2) - 1;
    if (len2 == 0) return s1;
    size_t new_len = len1 + len2;
    size_t new_sz = new_len + 1;
    lean_string_object* o = (lean_string_object*)lean_alloc_object(sizeof(lean_string_object) + new_sz);
    lean_set_st_header((lean_object*)o, LeanString, 0);
    o->m_size = new_sz;
    o->m_capacity = new_sz;
    o->m_length = lean_string_len(s1) + lean_string_len(s2);
    if (len1 > 0) rt_memcpy(o->m_data, lean_string_cstr(s1), len1);
    if (len2 > 0) rt_memcpy(o->m_data + len1, lean_string_cstr(s2), len2);
    o->m_data[new_len] = '\0';
    lean_dec(s1);
    return (lean_object*)o;
}

LEAN_EXPORT bool lean_string_eq_cold(b_lean_obj_arg s1, b_lean_obj_arg s2) {
    size_t sz1 = lean_string_size(s1);
    size_t sz2 = lean_string_size(s2);
    if (sz1 != sz2) return false;
    return rt_memcmp(lean_string_cstr(s1), lean_string_cstr(s2), sz1) == 0;
}

LEAN_EXPORT uint8_t lean_string_memcmp(
    b_lean_obj_arg s1,
    b_lean_obj_arg s2,
    b_lean_obj_arg lstart,
    b_lean_obj_arg rstart,
    b_lean_obj_arg len
) {
    size_t ls = lean_unbox(lstart);
    size_t rs = lean_unbox(rstart);
    size_t n = lean_unbox(len);
    size_t sz1 = lean_string_size(s1) - 1;
    size_t sz2 = lean_string_size(s2) - 1;
    if (ls + n > sz1 || rs + n > sz2) return 0;
    return rt_memcmp(lean_string_cstr(s1) + ls, lean_string_cstr(s2) + rs, n) == 0 ? 1 : 0;
}

static size_t encode_utf8_cp(uint32_t cp, char* out) {
    if (cp <= 0x7F) {
        out[0] = (char)cp;
        return 1;
    } else if (cp <= 0x7FF) {
        out[0] = (char)(0xC0 | (cp >> 6));
        out[1] = (char)(0x80 | (cp & 0x3F));
        return 2;
    } else if (cp <= 0xFFFF) {
        out[0] = (char)(0xE0 | (cp >> 12));
        out[1] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[2] = (char)(0x80 | (cp & 0x3F));
        return 3;
    } else {
        out[0] = (char)(0xF0 | (cp >> 18));
        out[1] = (char)(0x80 | ((cp >> 12) & 0x3F));
        out[2] = (char)(0x80 | ((cp >> 6) & 0x3F));
        out[3] = (char)(0x80 | (cp & 0x3F));
        return 4;
    }
}

LEAN_EXPORT lean_obj_res lean_string_mk(lean_obj_arg cs) {
    size_t char_count = 0;
    size_t byte_len = 0;
    for (lean_object* cur = cs; !lean_is_scalar(cur); cur = lean_ctor_get(cur, 1)) {
        uint32_t cp = lean_unbox_uint32(lean_ctor_get(cur, 0));
        char tmp[4];
        byte_len += encode_utf8_cp(cp, tmp);
        char_count++;
    }
    size_t sz = byte_len + 1;
    lean_string_object* o = (lean_string_object*)lean_alloc_object(sizeof(lean_string_object) + sz);
    lean_set_st_header((lean_object*)o, LeanString, 0);
    o->m_size = sz;
    o->m_capacity = sz;
    o->m_length = char_count;
    size_t pos = 0;
    for (lean_object* cur = cs; !lean_is_scalar(cur); cur = lean_ctor_get(cur, 1)) {
        uint32_t cp = lean_unbox_uint32(lean_ctor_get(cur, 0));
        pos += encode_utf8_cp(cp, o->m_data + pos);
    }
    o->m_data[byte_len] = '\0';
    lean_dec(cs);
    return (lean_object*)o;
}

LEAN_EXPORT lean_obj_res lean_string_data(lean_obj_arg s) {
    const uint8_t* bytes = (const uint8_t*)lean_string_cstr(s);
    size_t n = lean_string_size(s) - 1;
    /* Collect codepoints in an array then build list in reverse */
    uint32_t* cps = (uint32_t*)automatafl_rt_alloc(sizeof(uint32_t) * (n + 1));
    size_t count = 0;
    size_t i = 0;
    while (i < n) {
        uint8_t c = bytes[i];
        uint32_t cp = 0;
        if ((c & 0x80) == 0) {
            cp = c;
            i += 1;
        } else if ((c & 0xE0) == 0xC0 && i + 1 < n) {
            cp = ((uint32_t)(c & 0x1F) << 6) | (uint32_t)(bytes[i + 1] & 0x3F);
            i += 2;
        } else if ((c & 0xF0) == 0xE0 && i + 2 < n) {
            cp = ((uint32_t)(c & 0x0F) << 12) | ((uint32_t)(bytes[i + 1] & 0x3F) << 6) | (uint32_t)(bytes[i + 2] & 0x3F);
            i += 3;
        } else if (i + 3 < n) {
            cp = ((uint32_t)(c & 0x07) << 18) | ((uint32_t)(bytes[i + 1] & 0x3F) << 12) | ((uint32_t)(bytes[i + 2] & 0x3F) << 6) | (uint32_t)(bytes[i + 3] & 0x3F);
            i += 4;
        } else {
            cp = c;
            i += 1;
        }
        cps[count++] = cp;
    }
    lean_object* list = lean_box(0);
    while (count > 0) {
        count--;
        lean_object* cell = lean_alloc_ctor(1, 2, 0);
        lean_ctor_set(cell, 0, lean_box_uint32(cps[count]));
        lean_ctor_set(cell, 1, list);
        list = cell;
    }
    automatafl_rt_free(cps);
    lean_dec(s);
    return list;
}

LEAN_EXPORT uint32_t l_Char_ofNat(lean_object* n) {
    uint64_t v = nat_to_u64(n);
    if (v <= 0xD7FF || (v >= 0xE000 && v <= 0x10FFFF)) {
        return (uint32_t)v;
    }
    return 0;
}

LEAN_EXPORT lean_object* l_Nat_reprFast(lean_object* n) {
    uint64_t v = nat_to_u64(n);
    lean_dec(n);
    char buf[32];
    size_t pos = 32;
    if (v == 0) {
        buf[--pos] = '0';
    } else {
        while (v > 0) {
            buf[--pos] = (char)('0' + (v % 10));
            v /= 10;
        }
    }
    return mk_string_bytes(buf + pos, 32 - pos);
}

LEAN_EXPORT lean_object* l_Int_repr(lean_object* a) {
    int64_t v = int_to_i64(a);
    char buf[32];
    size_t pos = 32;
    bool neg = v < 0;
    uint64_t mag = neg ? ((uint64_t)(-(v + 1)) + 1u) : (uint64_t)v;
    if (mag == 0) {
        buf[--pos] = '0';
    } else {
        while (mag > 0) {
            buf[--pos] = (char)('0' + (mag % 10));
            mag /= 10;
        }
    }
    if (neg) {
        buf[--pos] = '-';
    }
    return mk_string_bytes(buf + pos, 32 - pos);
}

LEAN_EXPORT lean_object* l_String_Slice_toNat_x3f(lean_object* slice) {
    lean_object* s = lean_ctor_get(slice, 0);
    size_t start = lean_unbox(lean_ctor_get(slice, 1));
    size_t stop = lean_unbox(lean_ctor_get(slice, 2));
    size_t str_len = lean_string_size(s) - 1;
    if (stop > str_len) stop = str_len;
    if (start >= stop) return lean_box(0);
    const char* cstr = lean_string_cstr(s);
    uint64_t acc = 0;
    for (size_t i = start; i < stop; i++) {
        char c = cstr[i];
        if (c < '0' || c > '9') return lean_box(0);
        acc = acc * 10 + (uint64_t)(c - '0');
        if (acc > 0x7FFFFFFFULL) return lean_box(0);
    }
    lean_object* some = lean_alloc_ctor(1, 1, 0);
    lean_ctor_set(some, 0, alloc_mpz_u64(acc));
    return some;
}

LEAN_EXPORT lean_object* l_String_Slice_Pos_nextn(lean_object* slice, lean_object* pos, lean_object* n) {
    lean_object* s = lean_ctor_get(slice, 0);
    size_t base = lean_unbox(lean_ctor_get(slice, 1));
    size_t stop = lean_unbox(lean_ctor_get(slice, 2));
    size_t cur = base + lean_unbox(pos);
    size_t steps = lean_unbox(n);
    lean_dec(pos);
    const uint8_t* bytes = (const uint8_t*)lean_string_cstr(s);
    while (steps > 0 && cur < stop) {
        uint8_t b = bytes[cur];
        if ((b & 0x80) == 0) cur += 1;
        else if ((b & 0xE0) == 0xC0) cur += 2;
        else if ((b & 0xF0) == 0xE0) cur += 3;
        else cur += 4;
        if (cur > stop) cur = stop;
        steps--;
    }
    return lean_box(cur - base);
}

/* ==========================================================================
 * §7. List & Option Helpers (`l_List_*`, `l_Option_*`, `l_inst*`)
 * ========================================================================== */

LEAN_EXPORT lean_object* l_List_reverse___redArg(lean_object* l) {
    lean_object* acc = lean_box(0);
    while (!lean_is_scalar(l)) {
        lean_object* head = lean_ctor_get(l, 0);
        lean_object* tail = lean_ctor_get(l, 1);
        if (lean_is_exclusive(l)) {
            lean_ctor_set(l, 1, acc);
            acc = l;
            l = tail;
        } else {
            lean_inc(head);
            lean_inc(tail);
            lean_dec(l);
            lean_object* cell = lean_alloc_ctor(1, 2, 0);
            lean_ctor_set(cell, 0, head);
            lean_ctor_set(cell, 1, acc);
            acc = cell;
            l = tail;
        }
    }
    return acc;
}

LEAN_EXPORT lean_object* l_List_lengthTR___redArg(lean_object* l) {
    size_t len = 0;
    for (lean_object* cur = l; !lean_is_scalar(cur); cur = lean_ctor_get(cur, 1)) {
        len++;
    }
    return lean_box(len);
}

LEAN_EXPORT uint8_t l_List_isEmpty___redArg(lean_object* l) {
    return lean_is_scalar(l) ? 1 : 0;
}

LEAN_EXPORT uint8_t l_List_instDecidableEqNil___redArg(lean_object* l) {
    return lean_is_scalar(l) ? 1 : 0;
}

LEAN_EXPORT lean_object* l_List_head_x3f___redArg(lean_object* l) {
    if (lean_is_scalar(l)) return lean_box(0);
    lean_object* h = lean_ctor_get(l, 0);
    lean_inc(h);
    lean_object* some = lean_alloc_ctor(1, 1, 0);
    lean_ctor_set(some, 0, h);
    return some;
}

LEAN_EXPORT lean_object* l_List_appendTR___redArg(lean_object* as, lean_object* bs) {
    lean_object* rev = l_List_reverse___redArg(as);
    lean_object* acc = bs;
    while (!lean_is_scalar(rev)) {
        lean_object* head = lean_ctor_get(rev, 0);
        lean_object* tail = lean_ctor_get(rev, 1);
        if (lean_is_exclusive(rev)) {
            lean_ctor_set(rev, 1, acc);
            acc = rev;
            rev = tail;
        } else {
            lean_inc(head);
            lean_inc(tail);
            lean_dec(rev);
            lean_object* cell = lean_alloc_ctor(1, 2, 0);
            lean_ctor_set(cell, 0, head);
            lean_ctor_set(cell, 1, acc);
            acc = cell;
            rev = tail;
        }
    }
    return acc;
}

LEAN_EXPORT lean_object* l_List_range(lean_object* n_obj) {
    size_t n = lean_unbox(n_obj);
    lean_dec(n_obj);
    lean_object* acc = lean_box(0);
    while (n > 0) {
        n--;
        lean_object* cell = lean_alloc_ctor(1, 2, 0);
        lean_ctor_set(cell, 0, lean_box(n));
        lean_ctor_set(cell, 1, acc);
        acc = cell;
    }
    return acc;
}

LEAN_EXPORT uint8_t l_List_any___redArg(lean_object* l, lean_object* f) {
    while (!lean_is_scalar(l)) {
        lean_object* head = lean_ctor_get(l, 0);
        lean_object* tail = lean_ctor_get(l, 1);
        lean_inc(head);
        lean_inc(tail);
        lean_dec(l);
        lean_inc(f);
        lean_object* res = lean_apply_1(f, head);
        uint8_t b = lean_unbox(res);
        lean_dec(res);
        if (b != 0) {
            lean_dec(tail);
            lean_dec(f);
            return 1;
        }
        l = tail;
    }
    lean_dec(f);
    return 0;
}

LEAN_EXPORT uint8_t l_List_all___redArg(lean_object* l, lean_object* f) {
    while (!lean_is_scalar(l)) {
        lean_object* head = lean_ctor_get(l, 0);
        lean_object* tail = lean_ctor_get(l, 1);
        lean_inc(head);
        lean_inc(tail);
        lean_dec(l);
        lean_inc(f);
        lean_object* res = lean_apply_1(f, head);
        uint8_t b = lean_unbox(res);
        lean_dec(res);
        if (b == 0) {
            lean_dec(tail);
            lean_dec(f);
            return 0;
        }
        l = tail;
    }
    lean_dec(f);
    return 1;
}

LEAN_EXPORT lean_object* l_List_find_x3f___redArg(lean_object* f, lean_object* l) {
    while (!lean_is_scalar(l)) {
        lean_object* head = lean_ctor_get(l, 0);
        lean_object* tail = lean_ctor_get(l, 1);
        lean_inc(head);
        lean_inc(tail);
        lean_dec(l);
        lean_inc(f);
        lean_inc(head);
        lean_object* res = lean_apply_1(f, head);
        uint8_t b = lean_unbox(res);
        lean_dec(res);
        if (b != 0) {
            lean_dec(tail);
            lean_dec(f);
            lean_object* some = lean_alloc_ctor(1, 1, 0);
            lean_ctor_set(some, 0, head);
            return some;
        }
        lean_dec(head);
        l = tail;
    }
    lean_dec(f);
    return lean_box(0);
}

LEAN_EXPORT uint8_t l_List_elem___redArg(lean_object* beq, lean_object* a, lean_object* l) {
    while (!lean_is_scalar(l)) {
        lean_object* head = lean_ctor_get(l, 0);
        lean_object* tail = lean_ctor_get(l, 1);
        lean_inc(head);
        lean_inc(tail);
        lean_dec(l);
        lean_inc(beq);
        lean_inc(a);
        lean_object* res = lean_apply_2(beq, a, head);
        uint8_t b = lean_unbox(res);
        lean_dec(res);
        if (b != 0) {
            lean_dec(tail);
            lean_dec(a);
            lean_dec(beq);
            return 1;
        }
        l = tail;
    }
    lean_dec(a);
    lean_dec(beq);
    return 0;
}

LEAN_EXPORT uint8_t l_List_elem___at___00Lean_Meta_Grind_Arith_Cutsat_checkElimEqs_spec__0(lean_object* a, lean_object* l) {
    uint64_t target = nat_to_u64(a);
    for (lean_object* cur = l; !lean_is_scalar(cur); cur = lean_ctor_get(cur, 1)) {
        if (nat_to_u64(lean_ctor_get(cur, 0)) == target) {
            return 1;
        }
    }
    return 0;
}

LEAN_EXPORT lean_object* l_List_min_x3f___at___00Lean_Elab_Tactic_Omega_List_nonzeroMinimum_spec__1(lean_object* l) {
    if (lean_is_scalar(l)) return lean_box(0);
    uint64_t best = nat_to_u64(lean_ctor_get(l, 0));
    for (lean_object* cur = lean_ctor_get(l, 1); !lean_is_scalar(cur); cur = lean_ctor_get(cur, 1)) {
        uint64_t v = nat_to_u64(lean_ctor_get(cur, 0));
        if (v < best) best = v;
    }
    lean_object* some = lean_alloc_ctor(1, 1, 0);
    lean_ctor_set(some, 0, alloc_mpz_u64(best));
    return some;
}

LEAN_EXPORT lean_object* l_List_foldl___at___00Lean_Elab_Command_elabVersion_spec__0(lean_object* acc, lean_object* l) {
    for (lean_object* cur = l; !lean_is_scalar(cur); cur = lean_ctor_get(cur, 1)) {
        acc = lean_string_append(acc, lean_ctor_get(cur, 0));
    }
    return acc;
}

LEAN_EXPORT lean_object* l_List_foldl___at___00Array_appendList_spec__0___redArg(lean_object* arr, lean_object* l) {
    while (!lean_is_scalar(l)) {
        lean_object* head = lean_ctor_get(l, 0);
        lean_object* tail = lean_ctor_get(l, 1);
        lean_inc(head);
        lean_inc(tail);
        lean_dec(l);
        arr = lean_array_push(arr, head);
        l = tail;
    }
    return arr;
}

LEAN_EXPORT uint8_t l_List_nodupDecidable___redArg(lean_object* dec_eq, lean_object* l) {
    while (!lean_is_scalar(l)) {
        lean_object* head = lean_ctor_get(l, 0);
        lean_object* tail = lean_ctor_get(l, 1);
        lean_inc(head);
        lean_inc(tail);
        lean_dec(l);
        for (lean_object* cur = tail; !lean_is_scalar(cur); cur = lean_ctor_get(cur, 1)) {
            lean_object* elem = lean_ctor_get(cur, 0);
            lean_inc(dec_eq);
            lean_inc(head);
            lean_inc(elem);
            lean_object* eq = lean_apply_2(dec_eq, head, elem);
            uint8_t is_eq = lean_unbox(eq);
            lean_dec(eq);
            if (is_eq != 0) {
                lean_dec(head);
                lean_dec(tail);
                lean_dec(dec_eq);
                return 0;
            }
        }
        lean_dec(head);
        l = tail;
    }
    lean_dec(dec_eq);
    return 1;
}

LEAN_EXPORT lean_object* l_String_splitOnAux(
    lean_object* s,
    lean_object* sep,
    lean_object* b_obj,
    lean_object* i_obj,
    lean_object* j_obj,
    lean_object* r
) {
    const char* str = lean_string_cstr(s);
    size_t str_len = lean_string_size(s) - 1;
    const char* sp = lean_string_cstr(sep);
    size_t sep_len = lean_string_size(sep) - 1;
    size_t b = lean_unbox(b_obj);
    size_t i = lean_unbox(i_obj);
    (void)j_obj;
    lean_dec(b_obj);
    lean_dec(i_obj);
    lean_dec(j_obj);

    if (sep_len == 0) {
        lean_object* whole = mk_string_bytes(str, str_len);
        lean_object* cell = lean_alloc_ctor(1, 2, 0);
        lean_ctor_set(cell, 0, whole);
        lean_ctor_set(cell, 1, r);
        return l_List_reverse___redArg(cell);
    }

    while (i + sep_len <= str_len) {
        if (rt_memcmp(str + i, sp, sep_len) == 0) {
            lean_object* part = mk_string_bytes(str + b, i - b);
            lean_object* cell = lean_alloc_ctor(1, 2, 0);
            lean_ctor_set(cell, 0, part);
            lean_ctor_set(cell, 1, r);
            r = cell;
            i += sep_len;
            b = i;
        } else {
            i++;
        }
    }
    lean_object* tail_part = mk_string_bytes(str + b, str_len - b);
    lean_object* last_cell = lean_alloc_ctor(1, 2, 0);
    lean_ctor_set(last_cell, 0, tail_part);
    lean_ctor_set(last_cell, 1, r);
    return l_List_reverse___redArg(last_cell);
}

LEAN_EXPORT lean_object* l_instDecidableEqNat___boxed(lean_object* a, lean_object* b) {
    uint8_t eq = (nat_to_u64(a) == nat_to_u64(b)) ? 1 : 0;
    lean_dec(a);
    lean_dec(b);
    return lean_box(eq);
}

LEAN_EXPORT lean_object* l_instBEqOfDecidableEq___redArg___lam__0___boxed(lean_object* dec, lean_object* a, lean_object* b) {
    return lean_apply_2(dec, a, b);
}

LEAN_EXPORT uint8_t l_instDecidableEqProd___redArg(lean_object* decA, lean_object* decB, lean_object* p1, lean_object* p2) {
    lean_object* a1 = lean_ctor_get(p1, 0);
    lean_object* b1 = lean_ctor_get(p1, 1);
    lean_object* a2 = lean_ctor_get(p2, 0);
    lean_object* b2 = lean_ctor_get(p2, 1);
    lean_inc(a1); lean_inc(b1); lean_dec(p1);
    lean_inc(a2); lean_inc(b2); lean_dec(p2);
    lean_object* rA = lean_apply_2(decA, a1, a2);
    uint8_t eqA = lean_unbox(rA);
    lean_dec(rA);
    if (eqA == 0) {
        lean_dec(decB);
        lean_dec(b1);
        lean_dec(b2);
        return 0;
    }
    lean_object* rB = lean_apply_2(decB, b1, b2);
    uint8_t eqB = lean_unbox(rB);
    lean_dec(rB);
    return eqB;
}

LEAN_EXPORT uint8_t l_instDecidableEqList___redArg(lean_object* dec, lean_object* l1, lean_object* l2) {
    while (!lean_is_scalar(l1) && !lean_is_scalar(l2)) {
        lean_object* h1 = lean_ctor_get(l1, 0);
        lean_object* t1 = lean_ctor_get(l1, 1);
        lean_object* h2 = lean_ctor_get(l2, 0);
        lean_object* t2 = lean_ctor_get(l2, 1);
        lean_inc(h1); lean_inc(t1); lean_dec(l1);
        lean_inc(h2); lean_inc(t2); lean_dec(l2);
        lean_inc(dec);
        lean_object* r = lean_apply_2(dec, h1, h2);
        uint8_t eq = lean_unbox(r);
        lean_dec(r);
        if (eq == 0) {
            lean_dec(t1);
            lean_dec(t2);
            lean_dec(dec);
            return 0;
        }
        l1 = t1;
        l2 = t2;
    }
    uint8_t both_empty = (lean_is_scalar(l1) && lean_is_scalar(l2)) ? 1 : 0;
    lean_dec(l1);
    lean_dec(l2);
    lean_dec(dec);
    return both_empty;
}

LEAN_EXPORT uint8_t l_Option_decidableEqNone___redArg(lean_object* opt) {
    return lean_is_scalar(opt) ? 1 : 0;
}

LEAN_EXPORT uint8_t l_Option_instDecidableEq___redArg(lean_object* dec, lean_object* o1, lean_object* o2) {
    if (lean_is_scalar(o1) && lean_is_scalar(o2)) {
        lean_dec(dec);
        return 1;
    }
    if (lean_is_scalar(o1) || lean_is_scalar(o2)) {
        lean_dec(dec);
        lean_dec(o1);
        lean_dec(o2);
        return 0;
    }
    lean_object* v1 = lean_ctor_get(o1, 0);
    lean_object* v2 = lean_ctor_get(o2, 0);
    lean_inc(v1); lean_dec(o1);
    lean_inc(v2); lean_dec(o2);
    lean_object* r = lean_apply_2(dec, v1, v2);
    uint8_t eq = lean_unbox(r);
    lean_dec(r);
    return eq;
}

LEAN_EXPORT lean_object* l_Bool_repr___redArg(uint8_t b) {
    lean_object* s = b ? mk_string_bytes("true", 4) : mk_string_bytes("false", 5);
    lean_object* fmt = lean_alloc_ctor(3, 1, 0);
    lean_ctor_set(fmt, 0, s);
    return fmt;
}

LEAN_EXPORT lean_object* l_Repr_addAppParen(lean_object* fmt, lean_object* prec) {
    lean_dec(prec);
    return fmt;
}

LEAN_EXPORT lean_object* l_Std_Format_joinSep___at___00Prod_repr___at___00Array_repr___at___00__private_Lean_Elab_Structure_0__Lean_Elab_Command_Structure_resolveFieldDefaults_spec__2_spec__4_spec__5(
    lean_object* l,
    lean_object* sep
) {
    lean_dec(l);
    return sep;
}

/* ==========================================================================
 * §8. Module Initialization & Exported Rust FFI Entrypoints
 * ========================================================================== */

LEAN_EXPORT lean_object* initialize_Init(uint8_t builtin) {
    (void)builtin;
    return lean_io_result_mk_ok(lean_box(0));
}

LEAN_EXPORT lean_object* initialize_automatafl_Automatafl_Tactics(uint8_t builtin) {
    (void)builtin;
    return lean_io_result_mk_ok(lean_box(0));
}

extern lean_object* initialize_automatafl_Automatafl_ListAux(uint8_t builtin);
extern lean_object* initialize_automatafl_Automatafl_Base(uint8_t builtin);
extern lean_object* initialize_automatafl_Automatafl_Rules(uint8_t builtin);
extern lean_object* initialize_automatafl_Automatafl_Termination(uint8_t builtin);
extern lean_object* initialize_automatafl_Automatafl_Variants(uint8_t builtin);
extern lean_object* initialize_automatafl_Automatafl_PackedBoard(uint8_t builtin);
extern lean_object* initialize_automatafl_Automatafl_FFI(uint8_t builtin);

extern lean_object* dregg_automatafl_rules(lean_object* s);
extern lean_object* automatafl_packed_stock(uint32_t preset);
extern lean_object* automatafl_packed_step(lean_object* state_bytes, lean_object* move_bytes);
extern lean_object* automatafl_packed_legal_targets(lean_object* state_bytes, uint32_t fx, uint32_t fy, uint32_t live_only);
extern uint32_t     automatafl_packed_adjudicate(lean_object* state_bytes);

static lean_once_cell_t g_automatafl_init_cell = LEAN_ONCE_CELL_INITIALIZER;

LEAN_EXPORT void automatafl_lean_init(void) {
    if (LEAN_LIKELY(atomic_load_explicit(&g_automatafl_init_cell.state, memory_order_acquire) == 1)) {
        return;
    }
    once_cell_lock(&g_automatafl_init_cell);
    if (atomic_load_explicit(&g_automatafl_init_cell.state, memory_order_relaxed) == 0) {
        lean_dec(initialize_automatafl_Automatafl_ListAux(1));
        lean_dec(initialize_automatafl_Automatafl_Base(1));
        lean_dec(initialize_automatafl_Automatafl_Rules(1));
        lean_dec(initialize_automatafl_Automatafl_Termination(1));
        lean_dec(initialize_automatafl_Automatafl_Variants(1));
        lean_dec(initialize_automatafl_Automatafl_PackedBoard(1));
        lean_dec(initialize_automatafl_Automatafl_FFI(1));
        atomic_store_explicit(&g_automatafl_init_cell.state, 1, memory_order_release);
    }
    once_cell_unlock(&g_automatafl_init_cell);
}

static lean_object* mk_byte_array_from_slice(const uint8_t* ptr, size_t len) {
    lean_object* ba = lean_alloc_sarray(1, len, len);
    if (len > 0) {
        rt_memcpy(lean_sarray_cptr(ba), ptr, len);
    }
    return ba;
}

static size_t copy_out_byte_array(lean_object* ba, uint8_t* out_ptr, size_t out_cap) {
    size_t sz = lean_sarray_size(ba);
    size_t copy_n = (sz <= out_cap) ? sz : out_cap;
    if (copy_n > 0 && out_ptr != NULL) {
        rt_memcpy(out_ptr, lean_sarray_cptr(ba), copy_n);
    }
    lean_dec(ba);
    return sz;
}

LEAN_EXPORT size_t automatafl_lean_rules(
    const uint8_t* in_ptr,
    size_t in_len,
    uint8_t* out_ptr,
    size_t out_cap
) {
    automatafl_lean_init();
    lean_object* in_str = mk_string_bytes((const char*)in_ptr, in_len);
    lean_object* out_str = dregg_automatafl_rules(in_str);
    size_t sz = lean_string_size(out_str) - 1;
    size_t copy_n = (sz <= out_cap) ? sz : out_cap;
    if (copy_n > 0 && out_ptr != NULL) {
        rt_memcpy(out_ptr, lean_string_cstr(out_str), copy_n);
    }
    lean_dec(out_str);
    return sz;
}

LEAN_EXPORT size_t automatafl_lean_packed_stock(
    uint8_t preset,
    uint8_t* out_ptr,
    size_t out_cap
) {
    automatafl_lean_init();
    lean_object* ba = automatafl_packed_stock((uint32_t)preset);
    return copy_out_byte_array(ba, out_ptr, out_cap);
}

LEAN_EXPORT size_t automatafl_lean_packed_step(
    const uint8_t* state_ptr,
    size_t state_len,
    const uint8_t* moves_ptr,
    size_t moves_len,
    uint8_t* out_ptr,
    size_t out_cap
) {
    automatafl_lean_init();
    lean_object* state_ba = mk_byte_array_from_slice(state_ptr, state_len);
    lean_object* moves_ba = mk_byte_array_from_slice(moves_ptr, moves_len);
    lean_object* out_ba = automatafl_packed_step(state_ba, moves_ba);
    return copy_out_byte_array(out_ba, out_ptr, out_cap);
}

LEAN_EXPORT void automatafl_lean_packed_legal_targets(
    const uint8_t* state_ptr,
    size_t state_len,
    uint8_t who,
    uint8_t fx,
    uint8_t fy,
    uint8_t live_only,
    uint64_t* out_lo,
    uint64_t* out_hi
) {
    (void)who;
    automatafl_lean_init();
    unsigned board_size = (state_len >= 49) ? (unsigned)state_ptr[48] : 11u;
    lean_object* state_ba = mk_byte_array_from_slice(state_ptr, state_len);
    lean_object* out_ba = automatafl_packed_legal_targets(
        state_ba, (uint32_t)fx, (uint32_t)fy, (uint32_t)live_only);
    size_t sz = lean_sarray_size(out_ba);
    const uint8_t* d = lean_sarray_cptr(out_ba);
    uint64_t lo = 0, hi = 0;
    for (size_t i = 0; i + 1 < sz; i += 2) {
        unsigned tx = (unsigned)d[i];
        unsigned ty = (unsigned)d[i + 1];
        unsigned idx = ty * board_size + tx;
        if (idx < 64) {
            lo |= (1ULL << idx);
        } else if (idx < 128) {
            hi |= (1ULL << (idx - 64));
        }
    }
    *out_lo = lo;
    *out_hi = hi;
    lean_dec(out_ba);
}

LEAN_EXPORT uint32_t automatafl_lean_packed_adjudicate(
    const uint8_t* state_ptr,
    size_t state_len
) {
    automatafl_lean_init();
    lean_object* state_ba = mk_byte_array_from_slice(state_ptr, state_len);
    return automatafl_packed_adjudicate(state_ba);
}
