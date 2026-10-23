class Example
{
    int permits = 1;
    void Work(bool cancel)
    {
        permits -= 1;
        try
        {
            if (cancel) return;
        }
        finally
        {
            permits += 1;
        }
    }
}
