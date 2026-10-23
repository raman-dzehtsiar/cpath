class Example
{
    int value;
    int ExplicitField(int input)
    {
        this.value = input;
        this.value += 1;
        return this.value;
    }
    int Shadow(int value)
    {
        value = 1;
        value += 1;
        return value;
    }
}
