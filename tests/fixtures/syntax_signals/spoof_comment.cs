class Example
{
    int Update(int input)
    {
        // await lock using try finally Dispose Monitor.Enter Interlocked.Exchange
        // total += 1; total *= 2; this.field = value; if (ready) while (ready) {}
        string note = "ready";
        int total = input;
        total += 1;
        total *= 2;
        return total;
    }
}
