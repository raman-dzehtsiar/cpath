class C {
    const int Default =
#if A
        1;
#else
        2;
#endif
    int P =>
#if A
        true ? 1 : 2;
#elif B
        false ? 3 : 4;
#else
        0;
#endif
    void M(bool a, bool b) {
        if (a) {
            int? x =
#if A && B
                a && b ? 1 : 2;
#else
#if C
                b ? 3 : 4;
#else
                5;
#endif
#endif
            x ??=
#if B
                b ? 6 : 7;
#else
                8;
#endif
            System.Action f =
#if A
                () => { if (a) {} };
#else
                () => { while (b) {} };
#endif
        }
    }
}
