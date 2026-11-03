int f(int a, int b) { if (a) return 1; else
#if A
if (b) return 2;
#else
if (!b) return 3;
#endif
return 0; }
