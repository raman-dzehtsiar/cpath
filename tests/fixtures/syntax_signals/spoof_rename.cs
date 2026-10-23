class Example
{
    int Dispose(int asyncInput)
    {
        string lockNote = "ready";
        int awaitResult = asyncInput;
        awaitResult += 1;
        awaitResult *= 2;
        return awaitResult;
    }
}
