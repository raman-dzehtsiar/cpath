class Example
{
    int Work(int input)
    {
        int value = input;
        Step(ref value);
        Step(ref value);
        return value;
    }
    void Step(ref int value) { value += 1; }
}
