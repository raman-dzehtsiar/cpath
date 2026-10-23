class C { int M(bool a, bool b) { if (a) return 1; else
#if A
if (b) return 2;
#else
if (!b) return 3;
#endif
return 0; } }
