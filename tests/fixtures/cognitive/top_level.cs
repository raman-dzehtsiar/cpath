// SPDX-License-Identifier: MIT
using System;

int total = 0;
foreach (int value in new[] { 1, 2, 3 }) {
    if (value > 1) total += Measure(value);
}
Console.WriteLine(total);

int Measure(int value) {
    if (value > 2) return value * 2;
    return value;
}
