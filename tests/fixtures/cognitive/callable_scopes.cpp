template <typename T>
T choose(T a, T b) { return a ? a : b; }

struct Counter {
    int value;
    Counter(int n) : value(n > 0 ? n : 0) { if (n < 0) value = 0; }
    ~Counter() { if (value) value = 0; }
    int operator()(int n) const { return n ? value : 0; }
    explicit operator bool() const { return value > 0 && value < 10; }
    void run() {
        if (value) {
            auto task = [this](int n) { while (n) { if (value) --n; } };
            struct Local { int read(int n) { return n ? 1 : 0; } };
        }
    }
};

int guarded(int a, int b, int c) {
    try { if (a) throw a; }
    catch (int e) { if (b and c && a) return e; }
    catch (...) { return a ? 1 : 0; }
    return 0;
}

int range(int (&items)[3]) {
    for (auto value : items) { if (value) continue; }
    return 0;
}

int literals() {
    const char* text = R"tag(if (a && b) { while(c) {} })tag";
    // if (a) { goto label; }
    return 0;
}
