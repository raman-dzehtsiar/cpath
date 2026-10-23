class C { int M(bool a, bool b) {
#if A
if (a) return 1;
#else
if (b) return 2;
#endif
return 0;
} }
