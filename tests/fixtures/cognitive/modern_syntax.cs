// SPDX-License-Identifier: MIT
using System.Collections.Generic;
using System.Threading.Tasks;

namespace Corpus;

public record Sample(int Value);

class ModernSyntax {
    int Classify(object value) => value switch {
        int number when number > 0 => number > 10 ? 2 : 1,
        _ => 0
    };

    bool PatternOnly(int value) {
        if (value is > 0 and < 10) return true;
        return false;
    }

    async Task<int> CountAsync(IAsyncEnumerable<int> values) {
        int count = 0;
        await foreach (int value in values) {
            if (value > 0) count++;
        }
        return count;
    }

    string LiteralOnly() => """if (fake) { while (fake) {} } else {}""";

    string Interpolated(int value) => $"chosen: { (value > 0 ? value : 0) }";

    int NullSafe(Sample sample) => sample?.Value ?? 0;
}
