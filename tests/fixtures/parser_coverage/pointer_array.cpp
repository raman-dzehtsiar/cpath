void f(int n) { int **p = new int*[n ? 1 : 2]; delete[] p; }
