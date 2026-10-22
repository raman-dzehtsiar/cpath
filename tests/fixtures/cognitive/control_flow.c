int branches(int a, int b, int c) {
    if (a && b && c) {
        while (b) { if (c) break; }
    } else /* keep the chain flat */ if (b) {
        for (;;) { if (c) break; }
    } else { if (c) return 1; }
    return 0;
}

int choices(int a, int b) {
    switch (a) {
        case 1: if (b) goto done; break;
        default: return a ? (b ? 1 : 2) : 0;
    }
done:
    return 0;
}

int loops(int n) {
    for (int i = 0; i < n; ++i) { while (n) --n; }
    do { --n; } while (n > 0);
    return n;
}

int groups(int a, int b, int c, int d) {
    return (a && b) || (c && d);
}

#define CHOOSE(a, b) ((a) ? (b) : 0)
int configured(int a) {
#if FIRST && SECOND
    if (a) return 1;
#elif THIRD || FOURTH
    while (a) --a;
#else
    return CHOOSE(a, 2);
#endif
    return 0;
}
