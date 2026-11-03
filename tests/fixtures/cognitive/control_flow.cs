// SPDX-License-Identifier: MIT
using System;

class ControlFlow {
    int NestedBranches(bool ready, bool fallback, int[] values) {
        if (ready) {
            for (int i = 0; i < values.Length; i++) {
                if (values[i] < 0) return -1;
            }
        } else if (fallback) {
            return 1;
        } else {
            return 2;
        }
        return 0;
    }

    int SwitchCases(int value, bool retry) {
        switch (value) {
            case 1:
                if (retry) goto case 2;
                return 1;
            case 2: return 2;
            default: return 0;
        }
    }

    int TryCatch(bool ready, bool retry, bool enabled) {
        try {
            if (ready) return 1;
        } catch (Exception) when (retry && enabled) {
            if (retry) return 2;
        } finally {
            if (enabled) Console.WriteLine("done");
        }
        return 0;
    }

    bool BooleanGroups(bool a, bool b, bool c, bool d, bool e) {
        return (a && b && c) || (d && e);
    }

    int LoopKinds(int[] values) {
        int count = 0;
        foreach (int value in values) {
            while (count < value) count++;
        }
        do { count--; } while (count > 10);
        for (int i = 0; i < 2; i++) count++;
        return count;
    }

    int TernaryChain(bool a, bool b) => a ? (b ? 1 : 2) : 3;
}
