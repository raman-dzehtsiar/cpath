#define RETURN_IF(x) do { if (x) return 1; } while (0)
int f(int n) { RETURN_IF(n); return 0; }
