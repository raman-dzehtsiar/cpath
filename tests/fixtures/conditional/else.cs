class C {
    void M(bool a, bool b) {
        if (a && b) { while (b) {} }
#if A
        else if (b) { if (a) {} }
#elif B
#if C
        else if (a) { while (b) {} }
#endif
#endif
        else { if (a) {} }
        if (b) {}
#if D
        else { while (a) {} }
#else
        else if (a) {}
#endif
        if (a) {}
        else
#if X
        if (b) {}
#else
        while (a) {}
#endif
    }
}
