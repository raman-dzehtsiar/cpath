class C { int M(bool a) {
#if A
if (a) {
#else
if (!a) {
#endif
return 1;
}
return 0; } }
