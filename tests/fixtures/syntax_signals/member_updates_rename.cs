class Example
{
    int lockResult;
    int Dispose(int asyncInput)
    {
        this.lockResult = asyncInput;
        this.lockResult += 1;
        this.lockResult *= 2;
        return this.lockResult;
    }
}
