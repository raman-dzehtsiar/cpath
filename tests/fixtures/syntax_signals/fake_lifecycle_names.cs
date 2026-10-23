class Example
{
    void Update()
    {
        Acquire();
        Wait();
        Dispose();
        Release();
    }
    void Acquire() { }
    void Wait() { }
    void Dispose() { }
    void Release() { }
}
