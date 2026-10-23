class Example
{
    void Work()
    {
        Monitor.Enter();
        Monitor.Exit();
    }
}
static class Monitor
{
    public static void Enter() { }
    public static void Exit() { }
}
