class C :
#if A
    IA,
#elif B
#if C
    IB,
#endif
#endif
    IC
{
    int M(bool a, bool b) {
        var xs = new [] {
#if A && B
            a ? 1 : 2,
#elif C
            b ? 3 : 4,
#else
#endif
            a && b ? 5 : 6,
#if D
            a ? 7 : 8, b ? 9 : 10
#else
            11
#endif
        };
        try { if (a) F(); }
#if A && B
        catch (E) { if (b) F(); }
#elif C
#if D
        catch (E2) when (a || b) { while (b) F(); }
#endif
#endif
        finally { if (b) F(); }
        return 0;
    }
}
