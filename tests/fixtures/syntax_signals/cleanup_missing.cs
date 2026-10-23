class Example
{
    int permits = 1;
    void Work(bool cancel)
    {
        permits -= 1;
        if (cancel) return;
        permits += 1;
    }
}
